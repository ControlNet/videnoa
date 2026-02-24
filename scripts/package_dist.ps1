#!/usr/bin/env pwsh

[CmdletBinding()]
param(
    [string]$Repo = 'ControlNet/videnoa',
    [string]$ReleaseTag = 'misc',
    [ValidateSet('auto', 'linux64', 'win64')]
    [string]$Platform = 'win64',
    [string]$OutputDir = (Get-Location).Path,
    [string]$WorkDir = '',
    [string]$SourceDir = '',
    [switch]$KeepWorkDir,
    [switch]$Force,
    [switch]$Help
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Show-Usage {
    @'
Package Videnoa distribution folder (PowerShell).

This script will:
1) clone ControlNet/videnoa (or use -SourceDir)
2) run cargo build --release --workspace
3) download platform assets from GitHub release (lib/bin/models)
4) assemble a distribution folder named "videnoa"

Usage:
  scripts/package_dist.ps1 [options]

Options:
  -Repo <owner/name>       GitHub repository (default: ControlNet/videnoa)
  -ReleaseTag <tag>        Release tag for large assets (default: misc)
  -Platform <auto|linux64|win64>
                           Asset platform selector (default: win64)
  -OutputDir <path>        Parent directory for output folder "videnoa" (default: current directory)
  -WorkDir <path>          Working directory (default: temporary directory)
  -SourceDir <path>        Use a local source checkout instead of cloning from GitHub
  -KeepWorkDir             Keep temporary work directory after completion
  -Force                   Remove existing output "videnoa" folder if present
  -Help                    Show this help message

Examples:
  powershell -File scripts/package_dist.ps1
  powershell -File scripts/package_dist.ps1 -OutputDir .\dist -Force
  powershell -ExecutionPolicy Bypass -File scripts/package_dist.ps1
  powershell -ExecutionPolicy Bypass -File scripts/package_dist.ps1 -OutputDir .\dist -Force
  pwsh -File scripts/package_dist.ps1
  pwsh -File scripts/package_dist.ps1 -OutputDir .\dist -Force
  pwsh -File scripts/package_dist.ps1 -Platform win64 -ReleaseTag misc
  pwsh -File scripts/package_dist.ps1 -SourceDir C:\dev\videnoa -Force
'@ | Write-Output

    Write-Output 'Tip: -ExecutionPolicy Bypass is optional. If script execution is blocked, run once: Set-ExecutionPolicy -Scope CurrentUser RemoteSigned'
}

function Write-Log {
    param([Parameter(Mandatory = $true)][string]$Message)
    Write-Output "[package_dist] $Message"
}

function Write-WarnLog {
    param([Parameter(Mandatory = $true)][string]$Message)
    Write-Warning "[package_dist] $Message"
}

function Fail {
    param([Parameter(Mandatory = $true)][string]$Message)
    throw "[package_dist][error] $Message"
}

function Require-Command {
    param([Parameter(Mandatory = $true)][string]$Name)
    if (-not (Get-Command -Name $Name -ErrorAction SilentlyContinue)) {
        Fail "required command not found: $Name"
    }
}

function Enable-Tls12ForWebRequests {
    try {
        $current = [System.Net.ServicePointManager]::SecurityProtocol
        $tls12 = [System.Net.SecurityProtocolType]::Tls12

        if (($current -band $tls12) -eq 0) {
            [System.Net.ServicePointManager]::SecurityProtocol = $current -bor $tls12
            Write-Log 'enabled TLS 1.2 for web requests'
        }
    }
    catch {
        Write-WarnLog 'failed to adjust TLS settings; downloads may fail on older WinPS defaults'
    }
}

function Build-FrontendAssets {
    param([Parameter(Mandatory = $true)][string]$RepoRoot)

    $webDir = Join-Path -Path $RepoRoot -ChildPath 'web'
    if (-not (Test-Path -LiteralPath $webDir -PathType Container)) {
        Fail "missing frontend directory: $webDir"
    }

    $lockfilePath = Join-Path -Path $webDir -ChildPath 'package-lock.json'
    $installCmd = if (Test-Path -LiteralPath $lockfilePath -PathType Leaf) { 'ci' } else { 'install' }

    Write-Log "installing frontend dependencies (npm $installCmd --no-fund)"
    Push-Location $webDir
    try {
        & npm $installCmd --no-fund
        if ($LASTEXITCODE -ne 0) {
            Fail "npm $installCmd failed"
        }

        Write-Log 'building frontend assets (npm run build)'
        & npm run build
        if ($LASTEXITCODE -ne 0) {
            Fail 'npm run build failed'
        }
    }
    finally {
        Pop-Location
    }

    $distDir = Join-Path -Path $webDir -ChildPath 'dist'
    if (-not (Test-Path -LiteralPath $distDir -PathType Container)) {
        Fail "frontend build did not produce dist directory: $distDir"
    }
}

function New-TempDirectory {
    param([Parameter(Mandatory = $true)][string]$Prefix)
    $dirName = '{0}-{1}' -f $Prefix, ([System.Guid]::NewGuid().ToString('N').Substring(0, 10))
    $path = Join-Path -Path ([System.IO.Path]::GetTempPath()) -ChildPath $dirName
    New-Item -ItemType Directory -Path $path -Force | Out-Null
    return $path
}

function Resolve-Platform {
    param([Parameter(Mandatory = $true)][string]$InputPlatform)

    if ($InputPlatform -ne 'auto') {
        return $InputPlatform
    }

    $platformId = [System.Environment]::OSVersion.Platform
    if ($platformId -eq [System.PlatformID]::Win32NT) {
        return 'win64'
    }

    if ($platformId -eq [System.PlatformID]::Unix) {
        return 'linux64'
    }

    Fail "unsupported host platform. Use -Platform explicitly (linux64 or win64)."
}

function Get-RemoteContentLength {
    param([Parameter(Mandatory = $true)][string]$Uri)

    try {
        $response = Invoke-WebRequest -Uri $Uri -Method Head -MaximumRedirection 10 -ErrorAction Stop

        $headerValue = $null
        if ($response.Headers) {
            $headerValue = $response.Headers['Content-Length']
        }

        $parsedLength = 0L
        if ($headerValue -and [long]::TryParse([string]$headerValue, [ref]$parsedLength) -and $parsedLength -gt 0) {
            return [System.Nullable[long]]$parsedLength
        }

        if ($response.BaseResponse -and $response.BaseResponse.ContentLength -gt 0) {
            return [System.Nullable[long]]([long]$response.BaseResponse.ContentLength)
        }
    }
    catch {
        Write-WarnLog "unable to resolve Content-Length for $Uri; proceeding without size validation"
    }

    return $null
}

function Test-DownloadedAsset {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [System.Nullable[long]]$ExpectedLength = $null
    )

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        return $false
    }

    $actualLength = (Get-Item -LiteralPath $Path).Length
    if ($actualLength -le 0) {
        Write-WarnLog "downloaded file is empty: $Path"
        return $false
    }

    if ($ExpectedLength.HasValue -and $actualLength -ne $ExpectedLength.Value) {
        Write-WarnLog ("downloaded file size mismatch for {0}: expected {1} bytes, got {2} bytes" -f $Path, $ExpectedLength.Value, $actualLength)
        return $false
    }

    return $true
}

function Remove-DownloadArtifacts {
    param([Parameter(Mandatory = $true)][string]$Path)

    Remove-Item -LiteralPath $Path -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath ("{0}.aria2" -f $Path) -Force -ErrorAction SilentlyContinue
}

function Invoke-DownloadWithAria2 {
    param(
        [Parameter(Mandatory = $true)][string]$Uri,
        [Parameter(Mandatory = $true)][string]$OutputFile
    )

    $aria2 = Get-Command -Name 'aria2c' -ErrorAction SilentlyContinue
    if (-not $aria2) {
        return $false
    }

    $outputDir = Split-Path -Parent $OutputFile
    $outputName = Split-Path -Leaf $OutputFile

    & $aria2.Source '--allow-overwrite=true' '--auto-file-renaming=false' '--continue=true' '--max-tries=8' '--retry-wait=3' '--timeout=120' '--connect-timeout=30' '--max-connection-per-server=8' '--split=8' '--min-split-size=16M' '--summary-interval=0' '--console-log-level=warn' '--download-result=hide' '--file-allocation=none' '--dir' $outputDir '--out' $outputName $Uri | Out-Null
    return ($LASTEXITCODE -eq 0)
}

function Resolve-CurlCommand {
    $curlExe = Get-Command -Name 'curl.exe' -ErrorAction SilentlyContinue
    if ($curlExe) {
        return $curlExe
    }

    $curl = Get-Command -Name 'curl' -ErrorAction SilentlyContinue
    if ($curl -and $curl.CommandType -eq [System.Management.Automation.CommandTypes]::Application) {
        return $curl
    }

    return $null
}

function Invoke-DownloadWithCurl {
    param(
        [Parameter(Mandatory = $true)][string]$Uri,
        [Parameter(Mandatory = $true)][string]$OutputFile
    )

    $curl = Resolve-CurlCommand
    if (-not $curl) {
        return $false
    }

    & $curl.Source '--fail' '--location' '--silent' '--show-error' '--no-progress-meter' '--retry' '8' '--retry-all-errors' '--retry-delay' '2' '--connect-timeout' '30' '--speed-time' '30' '--speed-limit' '1024' '--continue-at' '-' '--output' $OutputFile $Uri
    return ($LASTEXITCODE -eq 0)
}

function Invoke-DownloadWithInvokeWebRequest {
    param(
        [Parameter(Mandatory = $true)][string]$Uri,
        [Parameter(Mandatory = $true)][string]$OutputFile
    )

    $previousProgressPreference = $ProgressPreference
    $ProgressPreference = 'SilentlyContinue'
    try {
        Invoke-WebRequest -Uri $Uri -OutFile $OutputFile -MaximumRedirection 10 -ErrorAction Stop
        return $true
    }
    catch {
        return $false
    }
    finally {
        $ProgressPreference = $previousProgressPreference
    }
}

function Test-ZipArchiveIntegrity {
    param([Parameter(Mandatory = $true)][string]$ZipFile)

    $sevenZip = Get-Command -Name '7z' -ErrorAction SilentlyContinue
    if (-not $sevenZip) {
        return $true
    }

    & 7z 't' $ZipFile | Out-Null
    return ($LASTEXITCODE -eq 0)
}

function Download-ReleaseAsset {
    param(
        [Parameter(Mandatory = $true)][string]$Repository,
        [Parameter(Mandatory = $true)][string]$Tag,
        [Parameter(Mandatory = $true)][string]$AssetName,
        [Parameter(Mandatory = $true)][string]$OutputFile
    )

    $releaseUrl = "https://github.com/$Repository/releases/download/$Tag/$AssetName"
    Write-Log "downloading asset: $AssetName"

    $expectedLength = Get-RemoteContentLength -Uri $releaseUrl
    if ($expectedLength.HasValue) {
        Write-Log ("expected size for {0}: {1} MiB" -f $AssetName, [math]::Round($expectedLength.Value / 1MB, 2))
    }

    $parent = Split-Path -Parent $OutputFile
    if ($parent) {
        New-Item -ItemType Directory -Path $parent -Force | Out-Null
    }

    $maxAttempts = 4
    for ($attempt = 1; $attempt -le $maxAttempts; $attempt++) {
        if (Test-DownloadedAsset -Path $OutputFile -ExpectedLength $expectedLength) {
            $existingBytes = (Get-Item -LiteralPath $OutputFile).Length
            Write-Log ("using existing downloaded asset {0}: {1:N1} MiB" -f $AssetName, ($existingBytes / 1MB))
            return
        }

        if ($expectedLength.HasValue -and (Test-Path -LiteralPath $OutputFile -PathType Leaf)) {
            $existingBytes = (Get-Item -LiteralPath $OutputFile).Length
            if ($existingBytes -gt $expectedLength.Value) {
                Write-WarnLog ("existing file larger than expected; deleting before retry: {0}" -f $OutputFile)
                Remove-DownloadArtifacts -Path $OutputFile
            }
        }

        $attemptStartedAt = Get-Date
        $backend = $null
        $downloaded = $false

        if (Invoke-DownloadWithAria2 -Uri $releaseUrl -OutputFile $OutputFile) {
            $backend = 'aria2c'
            $downloaded = $true
        }
        elseif (Invoke-DownloadWithCurl -Uri $releaseUrl -OutputFile $OutputFile) {
            $backend = 'curl'
            $downloaded = $true
        }
        elseif (Invoke-DownloadWithInvokeWebRequest -Uri $releaseUrl -OutputFile $OutputFile) {
            $backend = 'Invoke-WebRequest'
            $downloaded = $true
        }

        if ($downloaded -and (Test-DownloadedAsset -Path $OutputFile -ExpectedLength $expectedLength)) {
            $elapsedSeconds = [math]::Max(((Get-Date) - $attemptStartedAt).TotalSeconds, 0.01)
            $bytes = (Get-Item -LiteralPath $OutputFile).Length
            $sizeMiB = $bytes / 1MB
            $speedMiB = $sizeMiB / $elapsedSeconds
            Write-Log ("downloaded {0} using {1}: {2:N1} MiB in {3:N1}s ({4:N1} MiB/s)" -f $AssetName, $backend, $sizeMiB, $elapsedSeconds, $speedMiB)
            return
        }

        if ($attempt -lt $maxAttempts) {
            Write-WarnLog ("download attempt {0}/{1} failed for {2}; retrying" -f $attempt, $maxAttempts, $AssetName)
            Start-Sleep -Seconds ([math]::Min(2 * $attempt, 8))
        }
    }

    Fail "failed to download asset '$AssetName' from $releaseUrl after $maxAttempts attempts"
}

function Join-BinaryFiles {
    param(
        [Parameter(Mandatory = $true)][string[]]$InputFiles,
        [Parameter(Mandatory = $true)][string]$OutputFile
    )

    $parent = Split-Path -Parent $OutputFile
    if ($parent) {
        New-Item -ItemType Directory -Path $parent -Force | Out-Null
    }

    $outputStream = [System.IO.File]::Open($OutputFile, [System.IO.FileMode]::Create, [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
    try {
        foreach ($inputFile in $InputFiles) {
            $inputStream = [System.IO.File]::OpenRead($inputFile)
            try {
                $inputStream.CopyTo($outputStream)
            }
            finally {
                $inputStream.Dispose()
            }
        }
    }
    finally {
        $outputStream.Dispose()
    }
}

function Copy-DirectoryContents {
    param(
        [Parameter(Mandatory = $true)][string]$SourceDir,
        [Parameter(Mandatory = $true)][string]$DestinationDir
    )

    New-Item -ItemType Directory -Path $DestinationDir -Force | Out-Null
    $children = Get-ChildItem -LiteralPath $SourceDir -Force
    foreach ($child in $children) {
        Copy-Item -LiteralPath $child.FullName -Destination $DestinationDir -Recurse -Force
    }
}

function Expand-ZipArchivePortable {
    param(
        [Parameter(Mandatory = $true)][string]$ZipFile,
        [Parameter(Mandatory = $true)][string]$DestinationDir
    )

    $sevenZip = Get-Command -Name '7z' -ErrorAction SilentlyContinue
    if ($sevenZip) {
        & 7z 'x' ("-o{0}" -f $DestinationDir) '-y' $ZipFile | Out-Null
        if ($LASTEXITCODE -ne 0) {
            Fail "7z failed to extract archive: $ZipFile"
        }
        return
    }

    try {
        Expand-Archive -LiteralPath $ZipFile -DestinationPath $DestinationDir -Force
    }
    catch {
        Fail "failed to extract '$ZipFile' with Expand-Archive. Install 7-Zip and ensure '7z' is available in PATH"
    }
}

function Extract-ZipIntoDir {
    param(
        [Parameter(Mandatory = $true)][string]$ZipFile,
        [Parameter(Mandatory = $true)][string]$ExpectedRoot,
        [Parameter(Mandatory = $true)][string]$DestinationDir
    )

    $tempExtract = New-TempDirectory -Prefix 'videnoa-unzip'
    try {
        Expand-ZipArchivePortable -ZipFile $ZipFile -DestinationDir $tempExtract
        New-Item -ItemType Directory -Path $DestinationDir -Force | Out-Null

        $expectedPath = Join-Path -Path $tempExtract -ChildPath $ExpectedRoot
        if (Test-Path -LiteralPath $expectedPath -PathType Container) {
            Copy-DirectoryContents -SourceDir $expectedPath -DestinationDir $DestinationDir
            return
        }

        $entries = @(Get-ChildItem -LiteralPath $tempExtract -Force)
        if ($entries.Count -eq 1 -and $entries[0].PSIsContainer) {
            Copy-DirectoryContents -SourceDir $entries[0].FullName -DestinationDir $DestinationDir
            return
        }

        Copy-DirectoryContents -SourceDir $tempExtract -DestinationDir $DestinationDir
    }
    finally {
        if (Test-Path -LiteralPath $tempExtract) {
            Remove-Item -LiteralPath $tempExtract -Recurse -Force
        }
    }
}

function Validate-SourceTree {
    param([Parameter(Mandatory = $true)][string]$RepoRoot)

    $requiredFiles = @(
        'Cargo.toml',
        'crates/app/Cargo.toml',
        'web/package.json',
        'web/src/lib/utils.ts',
        'web/src/lib/runtime-desktop.ts',
        'web/src/lib/presentation-error.ts',
        'web/src/lib/presentation-format.ts'
    )

    $missing = @()
    foreach ($relativePath in $requiredFiles) {
        $fullPath = Join-Path -Path $RepoRoot -ChildPath $relativePath
        if (-not (Test-Path -LiteralPath $fullPath -PathType Leaf)) {
            $missing += $relativePath
        }
    }

    if ($missing.Count -gt 0) {
        Write-Error '[package_dist][error] source tree is missing required files:'
        foreach ($relativePath in $missing) {
            Write-Error "  - $relativePath"
        }
        Write-Error '[package_dist][error] this usually means the selected ref is missing recently added frontend files (or they were ignored and never committed).'
        Write-Error '[package_dist][error] fix by packaging from a local source checkout with -SourceDir, or commit/push missing files first.'
        exit 1
    }
}

function Validate-BundleLayout {
    param(
        [Parameter(Mandatory = $true)][string]$BundleDir,
        [Parameter(Mandatory = $true)][string]$BinaryName,
        [Parameter(Mandatory = $true)][string]$DesktopBinaryName
    )

    $expected = @($BinaryName, $DesktopBinaryName, 'lib', 'bin', 'models', 'presets', 'README.md', 'LICENSE')
    $ok = $true

    foreach ($entry in $expected) {
        $entryPath = Join-Path -Path $BundleDir -ChildPath $entry
        if (-not (Test-Path -LiteralPath $entryPath)) {
            Write-WarnLog "missing required entry: $entry"
            $ok = $false
        }
    }

    $entries = @(Get-ChildItem -LiteralPath $BundleDir -Force)
    foreach ($entry in $entries) {
        if ($expected -notcontains $entry.Name) {
            Write-WarnLog "unexpected extra entry: $($entry.Name)"
            $ok = $false
        }
    }

    foreach ($dirName in @('lib', 'bin', 'models', 'presets')) {
        $dirPath = Join-Path -Path $BundleDir -ChildPath $dirName
        if (-not (Test-Path -LiteralPath $dirPath -PathType Container)) {
            Write-WarnLog "required directory is missing or invalid: $dirName"
            $ok = $false
        }
    }

    if (-not $ok) {
        Fail 'bundle layout validation failed'
    }
}

if ($Help) {
    Show-Usage
    exit 0
}

Require-Command -Name 'git'
Require-Command -Name 'cargo'
Require-Command -Name 'npm'
Require-Command -Name 'Invoke-WebRequest'
Enable-Tls12ForWebRequests

$resolvedPlatform = Resolve-Platform -InputPlatform $Platform

switch ($resolvedPlatform) {
    'linux64' {
        $binAsset = 'bin_linux64.zip'
        $libPart1 = 'lib_linux64.zip.001'
        $libPart2 = 'lib_linux64.zip.002'
        $exeSuffix = ''
        $distBinaryName = 'videnoa'
        $distDesktopBinaryName = 'videnoa-desktop'
    }
    'win64' {
        $binAsset = 'bin_win64.zip'
        $libPart1 = 'lib_win64.zip.001'
        $libPart2 = 'lib_win64.zip.002'
        $exeSuffix = '.exe'
        $distBinaryName = 'videnoa.exe'
        $distDesktopBinaryName = 'videnoa-desktop.exe'
    }
    default {
        Fail "unsupported platform: $resolvedPlatform"
    }
}

if (-not (Test-Path -LiteralPath $OutputDir -PathType Container)) {
    New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
}
$resolvedOutputDir = (Resolve-Path -LiteralPath $OutputDir).Path

$workDirEphemeral = $false
$resolvedWorkDir = $null

try {
    if ([string]::IsNullOrWhiteSpace($WorkDir)) {
        $resolvedWorkDir = New-TempDirectory -Prefix 'videnoa-pack'
        $workDirEphemeral = $true
    }
    else {
        if (-not (Test-Path -LiteralPath $WorkDir -PathType Container)) {
            New-Item -ItemType Directory -Path $WorkDir -Force | Out-Null
        }
        $resolvedWorkDir = (Resolve-Path -LiteralPath $WorkDir).Path
    }

    $cloneDir = Join-Path -Path $resolvedWorkDir -ChildPath 'repo'
    $downloadDir = Join-Path -Path $resolvedWorkDir -ChildPath 'download'
    $bundleDir = Join-Path -Path $resolvedOutputDir -ChildPath 'videnoa'

    if (Test-Path -LiteralPath $bundleDir) {
        if ($Force) {
            Write-WarnLog "removing existing bundle directory: $bundleDir"
            Remove-Item -LiteralPath $bundleDir -Recurse -Force
        }
        else {
            Fail "output already exists: $bundleDir (use -Force to overwrite)"
        }
    }

    New-Item -ItemType Directory -Path $downloadDir -Force | Out-Null
    if (Test-Path -LiteralPath $cloneDir) {
        Remove-Item -LiteralPath $cloneDir -Recurse -Force
    }

    if (-not [string]::IsNullOrWhiteSpace($SourceDir)) {
        if (-not (Test-Path -LiteralPath $SourceDir -PathType Container)) {
            Fail "-SourceDir is not a directory: $SourceDir"
        }

        $resolvedSourceDir = (Resolve-Path -LiteralPath $SourceDir).Path
        if (-not (Test-Path -LiteralPath (Join-Path $resolvedSourceDir 'Cargo.toml') -PathType Leaf)) {
            Fail "-SourceDir does not look like videnoa repository root: $SourceDir"
        }

        Write-Log "copying source tree from local checkout: $resolvedSourceDir"
        New-Item -ItemType Directory -Path $cloneDir -Force | Out-Null
        Copy-DirectoryContents -SourceDir $resolvedSourceDir -DestinationDir $cloneDir
    }
    else {
        Write-Log "cloning repository: https://github.com/$Repo.git"
        & git clone --depth 1 "https://github.com/$Repo.git" $cloneDir
        if ($LASTEXITCODE -ne 0) {
            Fail 'git clone failed'
        }
    }

    Validate-SourceTree -RepoRoot $cloneDir

    Build-FrontendAssets -RepoRoot $cloneDir

    Write-Log 'building release workspace'
    Push-Location $cloneDir
    try {
        & cargo build --release --workspace
        if ($LASTEXITCODE -ne 0) {
            Fail 'cargo build failed'
        }
    }
    finally {
        Pop-Location
    }

    Write-Log "downloading release assets from tag '$ReleaseTag'"
    Download-ReleaseAsset -Repository $Repo -Tag $ReleaseTag -AssetName $binAsset -OutputFile (Join-Path $downloadDir $binAsset)
    Download-ReleaseAsset -Repository $Repo -Tag $ReleaseTag -AssetName $libPart1 -OutputFile (Join-Path $downloadDir $libPart1)
    Download-ReleaseAsset -Repository $Repo -Tag $ReleaseTag -AssetName $libPart2 -OutputFile (Join-Path $downloadDir $libPart2)
    Download-ReleaseAsset -Repository $Repo -Tag $ReleaseTag -AssetName 'models.zip' -OutputFile (Join-Path $downloadDir 'models.zip')

    Write-Log 'merging split lib archive'
    $mergedLibZip = Join-Path -Path $downloadDir -ChildPath ("lib_{0}.zip" -f $resolvedPlatform)
    Join-BinaryFiles -InputFiles @((Join-Path $downloadDir $libPart1), (Join-Path $downloadDir $libPart2)) -OutputFile $mergedLibZip

    if (-not (Test-ZipArchiveIntegrity -ZipFile $mergedLibZip)) {
        Write-WarnLog 'merged lib archive failed integrity check; re-downloading split assets once'
        Remove-DownloadArtifacts -Path (Join-Path $downloadDir $libPart1)
        Remove-DownloadArtifacts -Path (Join-Path $downloadDir $libPart2)
        Remove-DownloadArtifacts -Path $mergedLibZip

        Download-ReleaseAsset -Repository $Repo -Tag $ReleaseTag -AssetName $libPart1 -OutputFile (Join-Path $downloadDir $libPart1)
        Download-ReleaseAsset -Repository $Repo -Tag $ReleaseTag -AssetName $libPart2 -OutputFile (Join-Path $downloadDir $libPart2)
        Join-BinaryFiles -InputFiles @((Join-Path $downloadDir $libPart1), (Join-Path $downloadDir $libPart2)) -OutputFile $mergedLibZip

        if (-not (Test-ZipArchiveIntegrity -ZipFile $mergedLibZip)) {
            Fail "merged lib archive failed integrity check after retry: $mergedLibZip"
        }
    }

    foreach ($archiveName in @($binAsset, 'models.zip')) {
        $archivePath = Join-Path $downloadDir $archiveName
        if (-not (Test-ZipArchiveIntegrity -ZipFile $archivePath)) {
            Write-WarnLog ("archive failed integrity check, re-downloading once: {0}" -f $archiveName)
            Remove-DownloadArtifacts -Path $archivePath
            Download-ReleaseAsset -Repository $Repo -Tag $ReleaseTag -AssetName $archiveName -OutputFile $archivePath
            if (-not (Test-ZipArchiveIntegrity -ZipFile $archivePath)) {
                Fail "archive failed integrity check after retry: $archiveName"
            }
        }
    }

    Write-Log "assembling bundle directory: $bundleDir"
    New-Item -ItemType Directory -Path $bundleDir -Force | Out-Null

    $videnoaBinSrc = Join-Path -Path $cloneDir -ChildPath ("target/release/videnoa{0}" -f $exeSuffix)
    $videnoaDesktopBinSrc = Join-Path -Path $cloneDir -ChildPath ("target/release/videnoa-desktop{0}" -f $exeSuffix)

    if (-not (Test-Path -LiteralPath $videnoaBinSrc -PathType Leaf)) {
        Fail "missing build output: $videnoaBinSrc"
    }
    if (-not (Test-Path -LiteralPath $videnoaDesktopBinSrc -PathType Leaf)) {
        Fail "missing build output: $videnoaDesktopBinSrc"
    }

    Copy-Item -LiteralPath $videnoaBinSrc -Destination (Join-Path $bundleDir $distBinaryName) -Force
    Copy-Item -LiteralPath $videnoaDesktopBinSrc -Destination (Join-Path $bundleDir $distDesktopBinaryName) -Force

    Extract-ZipIntoDir -ZipFile $mergedLibZip -ExpectedRoot 'lib' -DestinationDir (Join-Path $bundleDir 'lib')
    Extract-ZipIntoDir -ZipFile (Join-Path $downloadDir $binAsset) -ExpectedRoot 'bin' -DestinationDir (Join-Path $bundleDir 'bin')
    Extract-ZipIntoDir -ZipFile (Join-Path $downloadDir 'models.zip') -ExpectedRoot 'models' -DestinationDir (Join-Path $bundleDir 'models')

    Copy-Item -LiteralPath (Join-Path $cloneDir 'presets') -Destination (Join-Path $bundleDir 'presets') -Recurse -Force
    Copy-Item -LiteralPath (Join-Path $cloneDir 'README.md') -Destination (Join-Path $bundleDir 'README.md') -Force
    Copy-Item -LiteralPath (Join-Path $cloneDir 'LICENSE') -Destination (Join-Path $bundleDir 'LICENSE') -Force

    Validate-BundleLayout -BundleDir $bundleDir -BinaryName $distBinaryName -DesktopBinaryName $distDesktopBinaryName

    Write-Log "bundle created successfully: $bundleDir"
}
finally {
    if ($resolvedWorkDir) {
        if ($workDirEphemeral) {
            if ($KeepWorkDir) {
                Write-Log "keeping work directory: $resolvedWorkDir"
            }
            elseif (Test-Path -LiteralPath $resolvedWorkDir) {
                Remove-Item -LiteralPath $resolvedWorkDir -Recurse -Force
            }
        }
        elseif ($KeepWorkDir) {
            Write-Log "keeping user-provided work directory: $resolvedWorkDir"
        }
    }
}
