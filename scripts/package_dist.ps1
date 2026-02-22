#!/usr/bin/env pwsh

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

[CmdletBinding()]
param(
    [string]$Repo = 'ControlNet/videnoa',
    [string]$ReleaseTag = 'misc',
    [ValidateSet('auto', 'linux64', 'win64')]
    [string]$Platform = 'auto',
    [string]$OutputDir = (Get-Location).Path,
    [string]$WorkDir = '',
    [string]$SourceDir = '',
    [switch]$KeepWorkDir,
    [switch]$Force,
    [switch]$Help
)

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
                           Asset platform selector (default: auto)
  -OutputDir <path>        Parent directory for output folder "videnoa" (default: current directory)
  -WorkDir <path>          Working directory (default: temporary directory)
  -SourceDir <path>        Use a local source checkout instead of cloning from GitHub
  -KeepWorkDir             Keep temporary work directory after completion
  -Force                   Remove existing output "videnoa" folder if present
  -Help                    Show this help message

Examples:
  pwsh -File scripts/package_dist.ps1
  pwsh -File scripts/package_dist.ps1 -OutputDir .\dist -Force
  pwsh -File scripts/package_dist.ps1 -Platform win64 -ReleaseTag misc
  pwsh -File scripts/package_dist.ps1 -SourceDir C:\dev\videnoa -Force
'@ | Write-Output
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

    if ($IsWindows) {
        return 'win64'
    }

    if ($IsLinux) {
        return 'linux64'
    }

    Fail "unsupported host platform. Use -Platform explicitly (linux64 or win64)."
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

    $parent = Split-Path -Parent $OutputFile
    if ($parent) {
        New-Item -ItemType Directory -Path $parent -Force | Out-Null
    }

    try {
        Invoke-WebRequest -Uri $releaseUrl -OutFile $OutputFile
    }
    catch {
        Fail "failed to download asset '$AssetName' from $releaseUrl"
    }
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

function Extract-ZipIntoDir {
    param(
        [Parameter(Mandatory = $true)][string]$ZipFile,
        [Parameter(Mandatory = $true)][string]$ExpectedRoot,
        [Parameter(Mandatory = $true)][string]$DestinationDir
    )

    $tempExtract = New-TempDirectory -Prefix 'videnoa-unzip'
    try {
        Expand-Archive -LiteralPath $ZipFile -DestinationPath $tempExtract -Force
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
    param([Parameter(Mandatory = $true)][string]$BundleDir)

    $expected = @('videnoa', 'videnoa-desktop', 'lib', 'bin', 'models', 'presets', 'README.md', 'LICENSE')
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
Require-Command -Name 'Invoke-WebRequest'
Require-Command -Name 'Expand-Archive'

$resolvedPlatform = Resolve-Platform -InputPlatform $Platform

switch ($resolvedPlatform) {
    'linux64' {
        $binAsset = 'bin_linux64.zip'
        $libPart1 = 'lib_linux64.zip.001'
        $libPart2 = 'lib_linux64.zip.002'
        $exeSuffix = ''
    }
    'win64' {
        $binAsset = 'bin_win64.zip'
        $libPart1 = 'lib_win64.zip.001'
        $libPart2 = 'lib_win64.zip.002'
        $exeSuffix = '.exe'
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

    Copy-Item -LiteralPath $videnoaBinSrc -Destination (Join-Path $bundleDir 'videnoa') -Force
    Copy-Item -LiteralPath $videnoaDesktopBinSrc -Destination (Join-Path $bundleDir 'videnoa-desktop') -Force

    Extract-ZipIntoDir -ZipFile $mergedLibZip -ExpectedRoot 'lib' -DestinationDir (Join-Path $bundleDir 'lib')
    Extract-ZipIntoDir -ZipFile (Join-Path $downloadDir $binAsset) -ExpectedRoot 'bin' -DestinationDir (Join-Path $bundleDir 'bin')
    Extract-ZipIntoDir -ZipFile (Join-Path $downloadDir 'models.zip') -ExpectedRoot 'models' -DestinationDir (Join-Path $bundleDir 'models')

    Copy-Item -LiteralPath (Join-Path $cloneDir 'presets') -Destination (Join-Path $bundleDir 'presets') -Recurse -Force
    Copy-Item -LiteralPath (Join-Path $cloneDir 'README.md') -Destination (Join-Path $bundleDir 'README.md') -Force
    Copy-Item -LiteralPath (Join-Path $cloneDir 'LICENSE') -Destination (Join-Path $bundleDir 'LICENSE') -Force

    Validate-BundleLayout -BundleDir $bundleDir

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
