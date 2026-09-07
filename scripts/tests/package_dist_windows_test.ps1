#!/usr/bin/env pwsh
param([Parameter(Mandatory = $true)][string]$FrontendDist)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repoRoot = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$packageScript = Join-Path $repoRoot 'scripts/package_dist.ps1'
$parseTokens = $null
$parseErrors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($packageScript, [ref]$parseTokens, [ref]$parseErrors)
if ($parseErrors.Count -ne 0) { throw ($parseErrors | Out-String) }

# Load only the production helpers; do not execute the packaging entry point.
foreach ($name in @('Build-FrontendAssets', 'Copy-DirectoryContents', 'Write-Log', 'Fail')) {
    $function = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    if ($null -eq $function) { throw "Missing packaging helper: $name" }
    Invoke-Expression $function.Extent.Text
}
$build = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.CommandAst] -and $node.GetCommandName() -eq 'cargo' -and $node.CommandElements[1].Extent.Text -eq 'build' }, $true)
$buildCommand = ($build.CommandElements | ForEach-Object { $_.Extent.Text }) -join ' '
$expectedCommand = 'cargo build --release --locked -p videnoa-app -p videnoa-desktop'
if ($buildCommand -ne $expectedCommand) { throw "Unexpected distribution build scope: $buildCommand" }
if (-not (Get-Content (Join-Path $repoRoot 'scripts/package_dist.sh') -Raw).Contains($expectedCommand)) {
    throw 'Linux and Windows distribution build scopes differ'
}

$source = (Resolve-Path -LiteralPath $FrontendDist).Path
if (-not (Test-Path -LiteralPath (Join-Path $source 'index.html') -PathType Leaf)) {
    throw 'This test requires real built frontend assets with index.html'
}
$temp = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString('N'))
try {
    New-Item -ItemType Directory -Path (Join-Path $temp 'web') -Force > $null
    Build-FrontendAssets -RepoRoot $temp -PrebuiltDist $source
    foreach ($file in Get-ChildItem -LiteralPath $source -Recurse -File) {
        $relative = [System.IO.Path]::GetRelativePath($source, $file.FullName)
        $copied = Join-Path (Join-Path $temp 'web/dist') $relative
        if ((Get-FileHash -LiteralPath $file.FullName).Hash -ne (Get-FileHash -LiteralPath $copied).Hash) {
            throw "Prebuilt asset content changed: $relative"
        }
    }
    $rejected = $false
    try { Build-FrontendAssets -RepoRoot $temp -PrebuiltDist (Join-Path $temp 'web') }
    catch {
        if ($_.Exception.Message -notmatch 'prebuilt frontend is missing index.html') { throw }
        $rejected = $true
    }
    if (-not $rejected) { throw 'Incomplete frontend assets were accepted' }
    Write-Output 'PASS: real prebuilt assets preserved, incomplete assets rejected, distribution build scope validated'
}
finally {
    if (Test-Path -LiteralPath $temp) { Remove-Item -LiteralPath $temp -Recurse -Force }
}
