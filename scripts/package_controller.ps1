#!/usr/bin/env pwsh

[CmdletBinding()]
param(
    [string]$Target = 'x86_64-pc-windows-msvc',
    [string]$SourceDir = (Split-Path -Parent $PSScriptRoot),
    [string]$OutputDir = (Get-Location).Path,
    [string]$BinaryPath = '',
    [string]$VerifyArchive = '',
    [switch]$Force,
    [switch]$Help
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Show-Usage {
    @'
Package the standalone Videnoa Controller Windows archive.

Usage:
  pwsh -File scripts/package_controller.ps1 [options]
  pwsh -File scripts/package_controller.ps1 -VerifyArchive <archive.zip>

Options:
  -Target <triple>       Rust target (default: x86_64-pc-windows-msvc)
  -SourceDir <path>      Repository root (default: script parent)
  -OutputDir <path>      Archive output directory (default: current directory)
  -BinaryPath <path>     Package an already-built Controller executable
  -VerifyArchive <path> Verify an existing Windows Controller archive
  -Force                 Replace an existing archive
  -Help                  Show this help message
'@ | Write-Output
}

function Write-Log {
    param([Parameter(Mandatory = $true)][string]$Message)
    Write-Output "[package_controller] $Message"
}

function Fail {
    param([Parameter(Mandatory = $true)][string]$Message)
    throw "[package_controller][error] $Message"
}

function Require-Command {
    param([Parameter(Mandatory = $true)][string]$Name)
    if (-not (Get-Command -Name $Name -ErrorAction SilentlyContinue)) {
        Fail "required command not found: $Name"
    }
}

function Get-WorkspaceVersion {
    param([Parameter(Mandatory = $true)][string]$RepoRoot)
    $manifest = Join-Path $RepoRoot 'Cargo.toml'
    if (-not (Test-Path -LiteralPath $manifest -PathType Leaf)) {
        Fail 'missing required file: Cargo.toml'
    }
    $match = Select-String -LiteralPath $manifest -Pattern '^version = "([^"]+)"$' | Select-Object -First 1
    if (-not $match -or $match.Matches.Count -ne 1) {
        Fail 'workspace version is missing or invalid in Cargo.toml'
    }
    $version = $match.Matches[0].Groups[1].Value
    if ($version -notmatch '^[0-9]+\.[0-9]+\.[0-9]+([-.][0-9A-Za-z.-]+)?$') {
        Fail 'workspace version is missing or invalid in Cargo.toml'
    }
    return $version
}

function Test-RequiredSourceFiles {
    param([Parameter(Mandatory = $true)][string]$RepoRoot)
    foreach ($name in @('controller.example.toml', 'README-controller.md', 'LICENSE')) {
        if (-not (Test-Path -LiteralPath (Join-Path $RepoRoot $name) -PathType Leaf)) {
            Fail "missing required file: $name"
        }
    }
}

function Get-ExpectedEntries {
    param([Parameter(Mandatory = $true)][string]$RootName)
    return @(
        "$RootName/",
        "$RootName/LICENSE",
        "$RootName/README-controller.md",
        "$RootName/controller.example.toml",
        "$RootName/videnoa-controller.exe"
    )
}

function Test-ControllerArchive {
    param([Parameter(Mandatory = $true)][string]$ArchivePath)
    if (-not (Test-Path -LiteralPath $ArchivePath -PathType Leaf)) {
        Fail "archive does not exist: $ArchivePath"
    }
    $filename = Split-Path -Leaf $ArchivePath
    if ($filename -notmatch '^videnoa-controller-v(.+)-windows-x86_64\.zip$') {
        Fail "archive filename does not match the Windows Controller contract: $filename"
    }
    $rootName = "videnoa-controller-v$($Matches[1])-windows-x86_64"
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [System.IO.Compression.ZipFile]::OpenRead($ArchivePath)
    try {
        $actual = @($archive.Entries | ForEach-Object { $_.FullName })
        $expected = Get-ExpectedEntries -RootName $rootName
        if (($actual -join "`n") -cne ($expected -join "`n")) {
            Fail 'unexpected archive member or ordering'
        }
        $forbidden = '(^|/)(models?|lib|bin|target|trt_cache|controller-web|dist|\.env|.*\.(onnx|engine|plan|dll|so([.][0-9]+)*|dylib|pdb|key|pem))(/|$)'
        if ($actual | Where-Object { $_ -match $forbidden }) {
            Fail 'archive contains forbidden GPU/model/runtime/cache/secret content'
        }
    }
    finally {
        $archive.Dispose()
    }
    Write-Log "archive layout verified: $ArchivePath"
}

function Test-WindowsBinary {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Version
    )
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        Fail "missing Controller executable: $Path"
    }
    $stream = [System.IO.File]::OpenRead($Path)
    try {
        if ($stream.ReadByte() -ne 0x4d -or $stream.ReadByte() -ne 0x5a) {
            Fail "Controller binary is not a Windows PE executable: $Path"
        }
    }
    finally {
        $stream.Dispose()
    }
    $binaryText = [System.Text.Encoding]::ASCII.GetString([System.IO.File]::ReadAllBytes($Path))
    if ($binaryText -match '(?i)(onnxruntime|cudnn|nvinfer|tensorrt|cudart)[^\x00]*\.dll') {
        Fail 'Controller binary references a forbidden GPU/runtime library'
    }
    if ([System.Environment]::OSVersion.Platform -eq [System.PlatformID]::Win32NT) {
        $actual = (& $Path --version 2>&1 | Out-String).Trim()
        if ($LASTEXITCODE -ne 0) {
            Fail "Controller binary version command failed: $Path --version"
        }
        $expected = "videnoa-controller $Version"
        if ($actual -cne $expected) {
            Fail "binary version mismatch: expected '$expected', got '$actual'"
        }
    }
    else {
        Fail 'Windows binary version validation requires a Windows host'
    }
}

function New-DeterministicZip {
    param(
        [Parameter(Mandatory = $true)][string]$ArchivePath,
        [Parameter(Mandatory = $true)][string]$RootName,
        [Parameter(Mandatory = $true)][string]$RepoRoot,
        [Parameter(Mandatory = $true)][string]$ExecutablePath
    )
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $epoch = [System.DateTimeOffset]::new(1980, 1, 1, 0, 0, 0, [System.TimeSpan]::Zero)
    $stream = [System.IO.File]::Open($ArchivePath, [System.IO.FileMode]::CreateNew)
    try {
        $archive = [System.IO.Compression.ZipArchive]::new($stream, [System.IO.Compression.ZipArchiveMode]::Create, $false)
        try {
            $rootEntry = $archive.CreateEntry("$RootName/", [System.IO.Compression.CompressionLevel]::NoCompression)
            $rootEntry.LastWriteTime = $epoch
            $items = @(
                [pscustomobject]@{ Name = 'LICENSE'; Path = (Join-Path $RepoRoot 'LICENSE') }
                [pscustomobject]@{ Name = 'README-controller.md'; Path = (Join-Path $RepoRoot 'README-controller.md') }
                [pscustomobject]@{ Name = 'controller.example.toml'; Path = (Join-Path $RepoRoot 'controller.example.toml') }
                [pscustomobject]@{ Name = 'videnoa-controller.exe'; Path = $ExecutablePath }
            )
            foreach ($item in $items) {
                $entry = $archive.CreateEntry("$RootName/$($item.Name)", [System.IO.Compression.CompressionLevel]::Optimal)
                $entry.LastWriteTime = $epoch
                $input = [System.IO.File]::OpenRead($item.Path)
                $output = $entry.Open()
                try {
                    $input.CopyTo($output)
                }
                finally {
                    $output.Dispose()
                    $input.Dispose()
                }
            }
        }
        finally {
            $archive.Dispose()
        }
    }
    finally {
        $stream.Dispose()
    }
}

if ($Help) {
    Show-Usage
    exit 0
}

if (-not [string]::IsNullOrWhiteSpace($VerifyArchive)) {
    Test-ControllerArchive -ArchivePath $VerifyArchive
    exit 0
}

if ($Target -cne 'x86_64-pc-windows-msvc') {
    Fail "unsupported Windows target: $Target"
}
if (-not (Test-Path -LiteralPath $SourceDir -PathType Container)) {
    Fail "source directory does not exist: $SourceDir"
}
$resolvedSource = (Resolve-Path -LiteralPath $SourceDir).Path
Test-RequiredSourceFiles -RepoRoot $resolvedSource
$version = Get-WorkspaceVersion -RepoRoot $resolvedSource
$rootName = "videnoa-controller-v$version-windows-x86_64"

if (-not (Test-Path -LiteralPath $OutputDir -PathType Container)) {
    New-Item -ItemType Directory -Path $OutputDir | Out-Null
}
$resolvedOutput = (Resolve-Path -LiteralPath $OutputDir).Path
$archivePath = Join-Path $resolvedOutput "$rootName.zip"
if (Test-Path -LiteralPath $archivePath) {
    if (-not $Force) {
        Fail "output already exists: $archivePath (use -Force to overwrite)"
    }
    Remove-Item -LiteralPath $archivePath -Force
}

if ([string]::IsNullOrWhiteSpace($BinaryPath)) {
    Require-Command -Name 'cargo'
    Write-Log "building release Controller for $Target"
    Push-Location $resolvedSource
    try {
        & cargo build --locked --release -p videnoa-controller --target $Target
        if ($LASTEXITCODE -ne 0) {
            Fail 'cargo build failed'
        }
    }
    finally {
        Pop-Location
    }
    $BinaryPath = Join-Path $resolvedSource "target/$Target/release/videnoa-controller.exe"
}
$resolvedBinary = (Resolve-Path -LiteralPath $BinaryPath).Path
Test-WindowsBinary -Path $resolvedBinary -Version $version
New-DeterministicZip -ArchivePath $archivePath -RootName $rootName -RepoRoot $resolvedSource -ExecutablePath $resolvedBinary
Test-ControllerArchive -ArchivePath $archivePath
Write-Log "archive created successfully: $archivePath"
