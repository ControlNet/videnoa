#!/usr/bin/env pwsh
#Requires -Version 7
<#
Build or verify the bundled media tools (FFmpeg, FFprobe, mkvmerge, mkvpropedit).

  -Build   Windows only. Assemble release asset misc/bin_win64.zip: download
           the pinned BtbN GPL FFmpeg build and the pinned official MKVToolNix
           portable zip and verify their SHA-256. Assemble bin/ with licenses,
           provenance and SHA256SUMS, write bin_win64.zip into -OutputDir,
           then run -Verify on the staged bin/ directory.
  -Verify  Check a bin/ directory (-BinDir) on Windows or Linux. The
           encoders and filters must be present, and the video pipeline's
           rawvideo -> scale(BT.709)/format/setparams -> encode path, the
           mkvmerge mux with source audio, statistics tagging and frame
           counts must work for libx264 and libx265.
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

# Pinned upstream inputs. Update each group together and record the change in
# .omo/knowledges when bumping FFmpeg or MKVToolNix.
# BtbN deletes daily autobuilds after about two weeks but keeps each month's
# last one, so pin a month-end autobuild.
$FfmpegRelease = 'autobuild-2026-09-30-13-08'
$FfmpegAsset = 'ffmpeg-n8.1.3-9-g29e619e767-win64-gpl-8.1.zip'
$FfmpegSha256 = '7b801cdd3a1a0bb54ae6f572187e68b4ed54f52086cfee133fab2180bfb429fa'
# Same MKVToolNix release as the Linux bundle; the SHA-256 matches the
# publisher's sha256sums.txt for that release.
$MkvtoolnixVersion = '101.0'
$MkvtoolnixAsset = 'mkvtoolnix-64-bit-101.0.zip'
$MkvtoolnixSha256 = '59a807b45b34ac524dfdef61edcf0642ee9bd6b08c130cfc078532c362569abc'

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
    $mkvmerge = Join-Path $Dir "mkvmerge$suffix"
    $mkvpropedit = Join-Path $Dir "mkvpropedit$suffix"
    foreach ($tool in @($ffmpeg, $ffprobe, $mkvmerge, $mkvpropedit)) {
        if (-not (Test-Path -LiteralPath $tool -PathType Leaf)) { Fail "missing $tool" }
    }

    $version = Invoke-Native $ffmpeg @('-hide_banner', '-version')
    Write-Log ($version -split "`n" | Select-Object -First 1)
    foreach ($flag in $RequiredBuildFlags) {
        if ($version -notmatch [regex]::Escape($flag)) { Fail "ffmpeg is not built with $flag" }
    }
    $null = Invoke-Native $ffprobe @('-hide_banner', '-version')
    Write-Log ((Invoke-Native $mkvmerge @('--version')).Trim())
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
        # Source whose audio mkvmerge adds to the encoded video, as VideoOutput does for MKV outputs.
        $source = Join-Path $scratch 'source.mkv'
        $null = Invoke-Native $ffmpeg @('-hide_banner', '-v', 'error', '-f', 'lavfi', '-i', 'testsrc2=size=64x36:rate=30',
            '-f', 'lavfi', '-i', 'sine=duration=1', '-frames:v', '30', '-c:v', 'libx264', '-c:a', 'aac', $source)
        foreach ($case in $EncodeCases) {
            $out = Join-Path $scratch "$($case.Codec)-$($case.PixelFormat).mkv"
            # Mirrors EncoderConfig::build_video_only_ffmpeg_args in crates/core/src/nodes/video_output.rs.
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

            # Mirrors EncoderConfig::build_mkvmerge_args: encoded video first, then the source without its video.
            $muxed = Join-Path $scratch "$($case.Codec)-$($case.PixelFormat)-muxed.mkv"
            $null = Invoke-Native $mkvmerge @('--quiet', '--output', $muxed, '--no-global-tags',
                '--default-track-flag', '0:yes', $out, '--no-video', $source)
            $muxProbe = Invoke-Native $ffprobe @('-v', 'error', '-show_entries', 'stream=codec_type,codec_name:stream_tags',
                '-of', 'json', $muxed) | ConvertFrom-Json
            $types = @($muxProbe.streams | ForEach-Object { $_.codec_type })
            $video = $muxProbe.streams[0]
            $videoTags = if ($video.PSObject.Properties['tags']) { $video.tags.PSObject.Properties } else { @() }
            $muxTagged = $videoTags | Where-Object { $_.Name -like 'NUMBER_OF_FRAMES*' } | Select-Object -First 1
            if (($types -join ',') -ne 'video,audio' -or $null -eq $muxTagged -or [int]$muxTagged.Value -ne 30) {
                Fail "unexpected mkvmerge output for $($case.Codec)/$($case.PixelFormat): $($muxProbe | ConvertTo-Json -Depth 5 -Compress)"
            }
            Write-Log "muxed $($case.Codec) $($case.PixelFormat) with source audio via mkvmerge"
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

    $mkvtoolnixZip = Join-Path $work $MkvtoolnixAsset
    Invoke-Download "https://mkvtoolnix.download/windows/releases/$MkvtoolnixVersion/$MkvtoolnixAsset" $mkvtoolnixZip
    $actual = (Get-FileHash -LiteralPath $mkvtoolnixZip -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $MkvtoolnixSha256) { Fail "SHA-256 mismatch for ${MkvtoolnixAsset}: expected $MkvtoolnixSha256, got $actual" }
    Write-Log "verified $MkvtoolnixAsset SHA-256 $actual"

    $ffmpegDir = Join-Path $work 'ffmpeg'
    $mkvtoolnixDir = Join-Path $work 'mkvtoolnix'
    Expand-Archive -LiteralPath $ffmpegZip -DestinationPath $ffmpegDir
    Expand-Archive -LiteralPath $mkvtoolnixZip -DestinationPath $mkvtoolnixDir
    $ffmpegRoot = Join-Path $ffmpegDir ([IO.Path]::GetFileNameWithoutExtension($FfmpegAsset))
    $mkvtoolnixRoot = Join-Path $mkvtoolnixDir 'mkvtoolnix'
    $mkvtoolnixTools = @((Join-Path $mkvtoolnixRoot 'mkvmerge.exe'), (Join-Path $mkvtoolnixRoot 'mkvpropedit.exe'))
    $mkvtoolnixLicense = Join-Path $mkvtoolnixRoot 'doc/COPYING.txt'
    foreach ($path in @((Join-Path $ffmpegRoot 'bin/ffmpeg.exe'), (Join-Path $ffmpegRoot 'bin/ffprobe.exe'), (Join-Path $ffmpegRoot 'LICENSE.txt')) + $mkvtoolnixTools + @($mkvtoolnixLicense)) {
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { Fail "missing expected input $path" }
    }

    $stage = Join-Path $work 'stage'
    $bin = Join-Path $stage 'bin'
    New-Item -ItemType Directory -Force -Path $bin | Out-Null
    Copy-Item -LiteralPath (Join-Path $ffmpegRoot 'bin/ffmpeg.exe'), (Join-Path $ffmpegRoot 'bin/ffprobe.exe') -Destination $bin
    Copy-Item -LiteralPath (Join-Path $ffmpegRoot 'LICENSE.txt') -Destination (Join-Path $bin 'FFMPEG-LICENSE.txt')
    Copy-Item -LiteralPath $mkvtoolnixTools -Destination $bin
    Copy-Item -LiteralPath $mkvtoolnixLicense -Destination (Join-Path $bin 'MKVTOOLNIX-LICENSE.txt')

    $ffmpegVersion = ((Invoke-Native (Join-Path $bin 'ffmpeg.exe') @('-hide_banner', '-version')) -split "`n" | Select-Object -First 1).Trim()
    $mkvVersion = (Invoke-Native (Join-Path $bin 'mkvmerge.exe') @('--version')).Trim()
    @(
        'Videnoa Windows media tools (bin_win64.zip)',
        '',
        "ffmpeg.exe, ffprobe.exe: $ffmpegVersion",
        "  Source: https://github.com/BtbN/FFmpeg-Builds/releases/tag/$FfmpegRelease",
        "  Asset: $FfmpegAsset (SHA-256 $FfmpegSha256)",
        '  GPL build (libx264, libx265, libzimg). License: FFMPEG-LICENSE.txt.',
        '  FFmpeg source: https://github.com/FFmpeg/FFmpeg; build scripts: https://github.com/BtbN/FFmpeg-Builds',
        '',
        "mkvmerge.exe, mkvpropedit.exe: $mkvVersion",
        "  Source: https://mkvtoolnix.download/windows/releases/$MkvtoolnixVersion/",
        "  Asset: $MkvtoolnixAsset (SHA-256 $MkvtoolnixSha256)",
        '  MKVToolNix, GPL-2.0. License: MKVTOOLNIX-LICENSE.txt. Project: https://mkvtoolnix.download/'
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
