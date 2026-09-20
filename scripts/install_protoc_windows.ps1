$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if (-not $IsWindows -or -not $env:RUNNER_TEMP -or -not $env:GITHUB_PATH) {
    throw 'This installer requires a Windows GitHub Actions runner.'
}

$version = '33.5'
$expectedHash = '7e3468cd1fbd1ae9361a5304d4ac28fbd593aa1a425b5464bd9d4da5fca224b4'
$archive = Join-Path $env:RUNNER_TEMP "protoc-$version-win64.zip"
$destination = Join-Path $env:RUNNER_TEMP "protoc-$version-win64"
$uri = "https://github.com/protocolbuffers/protobuf/releases/download/v$version/protoc-$version-win64.zip"

Invoke-WebRequest -Uri $uri -OutFile $archive
if ((Get-FileHash -Path $archive -Algorithm SHA256).Hash -ne $expectedHash) {
    throw 'The protoc archive SHA-256 does not match the pinned release.'
}
Expand-Archive -Path $archive -DestinationPath $destination

$bin = Join-Path $destination 'bin'
$protoc = Join-Path $bin 'protoc.exe'
$actualVersion = & $protoc --version
if ($LASTEXITCODE -ne 0 -or $actualVersion -ne "libprotoc $version") {
    throw "Unexpected protoc version: $actualVersion"
}
$bin | Out-File -FilePath $env:GITHUB_PATH -Encoding utf8 -Append
Write-Output "Installed $actualVersion"
