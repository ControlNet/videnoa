#!/usr/bin/env pwsh
#Requires -Version 7
<#
Build or verify the bundled media tools (FFmpeg, FFprobe, mkvpropedit).

  -Build   Windows only. Assemble release asset misc/bin_win64.zip: download
           the pinned BtbN GPL FFmpeg build and verify its SHA-256. Take
           mkvpropedit.exe from the current misc/bin_win64.zip. Assemble
           bin/ with licenses, provenance and SHA256SUMS, write bin_win64.zip
           into -OutputDir, then run -Verify on the staged bin/ directory.
  -Verify  Check a bin/ directory (-BinDir) on Windows or Linux. The
           encoders and filters must be present, and the video pipeline's
           rawvideo -> scale(BT.709)/format/setparams -> encode path, statistics
           tagging and frame counts must work for libx264 and libx265.
#>
[CmdletBinding()]
param(
    [switch]$Build,
    [switch]$Verify,
    [string]$BinDir = '',
    [string]$OutputDir = '',
    [string]$WorkDir = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

# Pinned upstream inputs. Update all three together and record the change in
# .omo/knowledges when bumping FFmpeg.
$FfmpegRelease = 'autobuild-2026-09-26-13-03'
$FfmpegAsset = 'ffmpeg-n8.1.3-win64-gpl-8.1.zip'
$FfmpegSha256 = 'd20ef03f0f4161453b9f46a72471eb5370f56b6410fe8bca14a4b0220c6c924a'
$CurrentBundleUrl = 'https://github.com/ControlNet/videnoa/releases/download/misc/bin_win64.zip'

$RequiredBuildFlags = @('--enable-gpl', '--enable-libx264', '--enable-libx265')
$RequiredEncoders = @('libx264', 'libx265', 'hevc_nvenc', 'h264_nvenc')
# The encode chain is scale (swscale) -> format -> setparams -> setsar; zscale is not used.
$RequiredFilters = @('scale', 'format', 'setparams', 'setsar')
# Codec/pixel-format pairs the UI can produce (libx265 defaults to 10-bit).
$EncodeCases = @(
    @{ Codec = 'libx264'; PixelFormat = 'yuv420p' },
    @{ Codec = 'libx264'; PixelFormat = 'yuv420p10le' },
    @{ Codec = 'libx265'; PixelFormat = 'yuv420p10le' },
    @{ Codec = 'libx265'; PixelFormat = 'yuv420p' }
)

function Write-Log([string]$Message) { Write-Output "[media_tools] $Message" }
function Fail([string]$Message) { throw "[media_tools][error] $Message" }

function Invoke-Native {
    param([string]$FilePath, [string[]]$Arguments)
    $output = & $FilePath @Arguments 2>&1
    if ($LASTEXITCODE -ne 0) {
        $text = ($output | Out-String).Trim()
        Fail "$([IO.Path]::GetFileName($FilePath)) $($Arguments -join ' ') exited with $LASTEXITCODE`n$text"
    }
    return ($output | Out-String)
}

function Invoke-Download([string]$Uri, [string]$OutFile) {
    Write-Log "downloading $Uri"
    for ($attempt = 1; $attempt -le 5; $attempt++) {
        try {
            Invoke-WebRequest -Uri $Uri -OutFile $OutFile -MaximumRetryCount 3 -RetryIntervalSec 5
            return
        }
        catch {
            if ($attempt -eq 5) { throw }
            Write-Warning "download attempt $attempt failed: $_"
            Start-Sleep -Seconds (5 * $attempt)
        }
    }
}

function Test-MediaTools([string]$Dir) {
    $suffix = if ($IsWindows) { '.exe' } else { '' }
    $ffmpeg = Join-Path $Dir "ffmpeg$suffix"
    $ffprobe = Join-Path $Dir "ffprobe$suffix"
    $mkvpropedit = Join-Path $Dir "mkvpropedit$suffix"
    foreach ($tool in @($ffmpeg, $ffprobe, $mkvpropedit)) {
        if (-not (Test-Path -LiteralPath $tool -PathType Leaf)) { Fail "missing $tool" }
    }

    $version = Invoke-Native $ffmpeg @('-hide_banner', '-version')
    Write-Log ($version -split "`n" | Select-Object -First 1)
    foreach ($flag in $RequiredBuildFlags) {
        if ($version -notmatch [regex]::Escape($flag)) { Fail "ffmpeg is not built with $flag" }
    }
    $null = Invoke-Native $ffprobe @('-hide_banner', '-version')
    Write-Log ((Invoke-Native $mkvpropedit @('--version')).Trim())

    $encoders = Invoke-Native $ffmpeg @('-hide_banner', '-encoders')
    foreach ($encoder in $RequiredEncoders) {
        if ($encoders -notmatch "(?m)^\s*V\S*\s+$([regex]::Escape($encoder))\s") { Fail "ffmpeg lacks encoder $encoder" }
    }
    $filters = Invoke-Native $ffmpeg @('-hide_banner', '-filters')
    foreach ($filter in $RequiredFilters) {
        if ($filters -notmatch "(?m)^\s*\S+\s+$([regex]::Escape($filter))\s") { Fail "ffmpeg lacks filter $filter" }
    }
    Write-Log "encoders present: $($RequiredEncoders -join ', ') (NVENC needs a GPU and is not exercised)"

    $scratch = Join-Path ([IO.Path]::GetTempPath()) "videnoa-media-verify-$([guid]::NewGuid().ToString('N'))"
    New-Item -ItemType Directory -Path $scratch | Out-Null
    try {
        # Videnoa pipes packed RGB frames into FFmpeg; use the same input form.
        $raw = Join-Path $scratch 'frames.rgb'
        $null = Invoke-Native $ffmpeg @('-hide_banner', '-v', 'error', '-f', 'lavfi', '-i', 'testsrc2=size=128x72:rate=30',
            '-frames:v', '30', '-f', 'rawvideo', '-pix_fmt', 'rgb24', $raw)
        foreach ($case in $EncodeCases) {
            $out = Join-Path $scratch "$($case.Codec)-$($case.PixelFormat).mkv"
            # Mirrors EncoderConfig::build_ffmpeg_args in crates/core/src/nodes/video_output.rs.
            $vf = 'scale=flags=bicubic:out_color_matrix=bt709:out_range=limited,' +
                "format=$($case.PixelFormat)," +
                'setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709:range=limited,setsar=1'
            $null = Invoke-Native $ffmpeg @('-hide_banner', '-v', 'error', '-nostdin', '-y',
                '-f', 'rawvideo', '-pix_fmt', 'rgb24', '-s', '128x72', '-r', '30', '-i', $raw,
                '-vf', $vf, '-c:v', $case.Codec, '-crf', '18', '-preset', 'ultrafast', '-pix_fmt', $case.PixelFormat, $out)
            $null = Invoke-Native $mkvpropedit @($out, '--add-track-statistics-tags')
            $probe = Invoke-Native $ffprobe @('-v', 'error', '-count_frames', '-select_streams', 'v:0',
                '-show_entries', 'stream=codec_name,pix_fmt,nb_read_frames:stream_tags', '-of', 'json', $out) |
                ConvertFrom-Json
            $stream = $probe.streams[0]
            # Matroska statistics tags may carry a language suffix (NUMBER_OF_FRAMES-eng).
            $tags = if ($stream.PSObject.Properties['tags']) { $stream.tags.PSObject.Properties } else { @() }
            $tagged = $tags | Where-Object { $_.Name -like 'NUMBER_OF_FRAMES*' } | Select-Object -First 1
            if ([int]$stream.nb_read_frames -ne 30 -or $null -eq $tagged -or [int]$tagged.Value -ne 30 -or $stream.pix_fmt -ne $case.PixelFormat) {
                Fail "unexpected probe for $($case.Codec)/$($case.PixelFormat): $($probe | ConvertTo-Json -Depth 5 -Compress)"
            }
            Write-Log "encoded $($case.Codec) $($case.PixelFormat): 30 frames, statistics tag 30"
        }
    }
    finally {
        Remove-Item -LiteralPath $scratch -Recurse -Force -ErrorAction SilentlyContinue
    }
    Write-Log "verified media tools in $Dir"
}

function New-MediaToolsBundle {
    if (-not $IsWindows) { Fail '-Build assembles the Windows bundle and must run on Windows' }
    if (-not $OutputDir) { Fail '-OutputDir is required with -Build' }
    $work = if ($WorkDir) { $WorkDir } else { Join-Path ([IO.Path]::GetTempPath()) "videnoa-media-build-$([guid]::NewGuid().ToString('N'))" }
    New-Item -ItemType Directory -Force -Path $work, $OutputDir | Out-Null

    $ffmpegZip = Join-Path $work $FfmpegAsset
    Invoke-Download "https://github.com/BtbN/FFmpeg-Builds/releases/download/$FfmpegRelease/$FfmpegAsset" $ffmpegZip
    $actual = (Get-FileHash -LiteralPath $ffmpegZip -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $FfmpegSha256) { Fail "SHA-256 mismatch for ${FfmpegAsset}: expected $FfmpegSha256, got $actual" }
    Write-Log "verified $FfmpegAsset SHA-256 $actual"

    $currentZip = Join-Path $work 'current-bin_win64.zip'
    Invoke-Download $CurrentBundleUrl $currentZip

    $ffmpegDir = Join-Path $work 'ffmpeg'
    $currentDir = Join-Path $work 'current'
    Expand-Archive -LiteralPath $ffmpegZip -DestinationPath $ffmpegDir
    Expand-Archive -LiteralPath $currentZip -DestinationPath $currentDir
    $ffmpegRoot = Join-Path $ffmpegDir ([IO.Path]::GetFileNameWithoutExtension($FfmpegAsset))
    $currentMkvpropedit = Join-Path $currentDir 'bin/mkvpropedit.exe'
    foreach ($path in @((Join-Path $ffmpegRoot 'bin/ffmpeg.exe'), (Join-Path $ffmpegRoot 'bin/ffprobe.exe'), (Join-Path $ffmpegRoot 'LICENSE.txt'), $currentMkvpropedit)) {
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { Fail "missing expected input $path" }
    }

    $stage = Join-Path $work 'stage'
    $bin = Join-Path $stage 'bin'
    New-Item -ItemType Directory -Force -Path $bin | Out-Null
    Copy-Item -LiteralPath (Join-Path $ffmpegRoot 'bin/ffmpeg.exe'), (Join-Path $ffmpegRoot 'bin/ffprobe.exe') -Destination $bin
    Copy-Item -LiteralPath (Join-Path $ffmpegRoot 'LICENSE.txt') -Destination (Join-Path $bin 'FFMPEG-LICENSE.txt')
    Copy-Item -LiteralPath $currentMkvpropedit -Destination $bin

    $ffmpegVersion = ((Invoke-Native (Join-Path $bin 'ffmpeg.exe') @('-hide_banner', '-version')) -split "`n" | Select-Object -First 1).Trim()
    $mkvVersion = (Invoke-Native (Join-Path $bin 'mkvpropedit.exe') @('--version')).Trim()
    $mkvSha = (Get-FileHash -LiteralPath $currentMkvpropedit -Algorithm SHA256).Hash.ToLowerInvariant()
    @(
        'Videnoa Windows media tools (bin_win64.zip)',
        '',
        "ffmpeg.exe, ffprobe.exe: $ffmpegVersion",
        "  Source: https://github.com/BtbN/FFmpeg-Builds/releases/tag/$FfmpegRelease",
        "  Asset: $FfmpegAsset (SHA-256 $FfmpegSha256)",
        '  GPL build (libx264, libx265, libzimg). License: FFMPEG-LICENSE.txt.',
        '  FFmpeg source: https://github.com/FFmpeg/FFmpeg; build scripts: https://github.com/BtbN/FFmpeg-Builds',
        '',
        "mkvpropedit.exe: $mkvVersion",
        "  Carried over unchanged from the previous bin_win64.zip (SHA-256 $mkvSha).",
        '  MKVToolNix, GPL-2.0: https://mkvtoolnix.download/'
    ) | Set-Content -LiteralPath (Join-Path $bin 'README-windows-tools.txt') -Encoding utf8

    $sums = Get-ChildItem -LiteralPath $bin -File | Sort-Object Name | ForEach-Object {
        "$((Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant())  $($_.Name)"
    }
    $sums | Set-Content -LiteralPath (Join-Path $bin 'SHA256SUMS') -Encoding ascii

    Test-MediaTools $bin

    $zip = Join-Path $OutputDir 'bin_win64.zip'
    Remove-Item -LiteralPath $zip -Force -ErrorAction SilentlyContinue
    Compress-Archive -LiteralPath $bin -DestinationPath $zip -CompressionLevel Optimal
    $zipSha = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
    "$zipSha  bin_win64.zip" | Set-Content -LiteralPath (Join-Path $OutputDir 'bin_win64.zip.sha256') -Encoding ascii
    Write-Log "wrote $zip ($((Get-Item -LiteralPath $zip).Length) bytes, SHA-256 $zipSha)"

    # Round-trip: the packaged archive must extract to bin/ and verify again.
    $roundTrip = Join-Path $work 'roundtrip'
    Expand-Archive -LiteralPath $zip -DestinationPath $roundTrip
    Test-MediaTools (Join-Path $roundTrip 'bin')
}

if ($Build -eq $Verify) { Fail 'pass exactly one of -Build or -Verify' }
if ($Build) { New-MediaToolsBundle }
else {
    if (-not $BinDir) { Fail '-BinDir is required with -Verify' }
    Test-MediaTools $BinDir
}
