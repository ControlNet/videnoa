#!/usr/bin/env python3
from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import cast

from PIL import Image, UnidentifiedImageError

ICO_ENTRY_SIZES = (16, 24, 32, 48, 64, 128, 256)
PNG_COMPRESS_LEVEL = 9

ROOT_DIR = Path(__file__).resolve().parent.parent


@dataclass(frozen=True)
class TargetConfig:
    png: tuple[tuple[int, Path], ...]
    ico: tuple[Path, ...]
    svg: tuple[Path, ...] = ()


@dataclass(frozen=True)
class CliArgs:
    source: str
    targets: list[str]


MANAGED_TARGETS: dict[str, TargetConfig] = {
    "web": TargetConfig(
        png=(
            (16, Path("web/public/favicon-16x16.png")),
            (32, Path("web/public/favicon-32x32.png")),
            (512, Path("web/public/videnoa_icon.png")),
        ),
        ico=(Path("web/public/favicon.ico"),),
        svg=(Path("web/public/favicon.svg"),),
    ),
    "desktop": TargetConfig(
        png=(
            (512, Path("crates/desktop/icons/icon.png")),
            (32, Path("crates/desktop/icons/icon-32.png")),
            (64, Path("crates/desktop/icons/icon-64.png")),
            (128, Path("crates/desktop/icons/icon-128.png")),
            (256, Path("crates/desktop/icons/icon-256.png")),
        ),
        ico=(Path("crates/desktop/icons/icon.ico"),),
    ),
}


def _resample_filter() -> int:
    if hasattr(Image, "Resampling"):
        return Image.Resampling.LANCZOS
    return int(getattr(Image, "LANCZOS", 1))


RESAMPLE_FILTER = _resample_filter()


def parse_targets(value: str) -> list[str]:
    raw_targets = [segment.strip() for segment in value.split(",") if segment.strip()]
    if not raw_targets:
        raise argparse.ArgumentTypeError("--targets must include at least one target")

    known = set(MANAGED_TARGETS.keys())
    unknown = sorted(set(raw_targets) - known)
    if unknown:
        supported = ",".join(sorted(known))
        unknown_str = ",".join(unknown)
        raise argparse.ArgumentTypeError(
            f"unknown target(s): {unknown_str}. Supported targets: {supported}"
        )

    deduped: list[str] = []
    seen: set[str] = set()
    for target in raw_targets:
        if target in seen:
            continue
        seen.add(target)
        deduped.append(target)
    return deduped


def parse_args() -> CliArgs:
    parser = argparse.ArgumentParser(
        description="Deterministically generate managed Web/Desktop icon outputs.",
    )
    _ = parser.add_argument(
        "--source",
        required=True,
        help="Path to source image file (assets/icon.svg preferred).",
    )
    _ = parser.add_argument(
        "--targets",
        required=True,
        type=parse_targets,
        help="Comma-separated targets to sync: web,desktop",
    )
    namespace = parser.parse_args()
    return CliArgs(
        source=cast(str, namespace.source),
        targets=cast(list[str], namespace.targets),
    )


def resolve_source(source_arg: str) -> Path:
    source_path = Path(source_arg).expanduser()
    return source_path.resolve()


def load_source_image(source_path: Path) -> Image.Image:
    if not source_path.exists():
        raise ValueError(f"source file does not exist: {source_path}")
    if not source_path.is_file():
        raise ValueError(f"source path is not a file: {source_path}")

    suffix = source_path.suffix.lower()
    if suffix == ".png":
        rgba = load_png_image(source_path)
    elif suffix == ".svg":
        rgba = load_svg_image(source_path)
    else:
        raise ValueError(f"source must be a .svg or .png image: {source_path}")

    if rgba.width != rgba.height:
        raise ValueError(
            f"source image must be square, got {rgba.width}x{rgba.height}: {source_path}"
        )

    normalized = Image.frombytes("RGBA", rgba.size, rgba.tobytes())
    return normalized


def load_png_image(source_path: Path) -> Image.Image:
    try:
        with Image.open(source_path) as image:
            _ = image.load()
            if image.format != "PNG":
                raise ValueError(f"source must be a PNG image: {source_path}")
            return image.convert("RGBA")
    except UnidentifiedImageError as error:
        raise ValueError(f"source is not a readable image: {source_path}") from error
    except OSError as error:
        raise ValueError(f"cannot read source image: {source_path}: {error}") from error


def load_svg_image(source_path: Path) -> Image.Image:
    convert_bin = shutil.which("convert")
    if not convert_bin:
        raise ValueError("source is SVG but ImageMagick 'convert' is not installed")

    with tempfile.TemporaryDirectory(prefix="videnoa-icon-") as temp_dir:
        rendered_png = Path(temp_dir) / "source-rendered.png"
        cmd = [
            convert_bin,
            str(source_path),
            "-background",
            "none",
            "-resize",
            "1024x1024",
            f"PNG32:{rendered_png}",
        ]
        try:
            _ = subprocess.run(
                cmd, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE
            )
        except subprocess.CalledProcessError as error:
            raise ValueError("cannot rasterize SVG source") from error

        try:
            with Image.open(rendered_png) as image:
                _ = image.load()
                return image.convert("RGBA")
        except UnidentifiedImageError as error:
            raise ValueError(
                f"rendered SVG is not a readable image: {source_path}"
            ) from error
        except OSError as error:
            raise ValueError(
                f"cannot read rendered SVG image: {source_path}: {error}"
            ) from error


def save_png(source: Image.Image, size: int, output_path: Path) -> None:
    output_path.parent.mkdir(parents=True, exist_ok=True)
    rendered = source.resize((size, size), RESAMPLE_FILTER)
    rendered.save(
        output_path,
        format="PNG",
        optimize=False,
        compress_level=PNG_COMPRESS_LEVEL,
    )


def save_ico(source: Image.Image, output_path: Path) -> None:
    output_path.parent.mkdir(parents=True, exist_ok=True)
    source.save(
        output_path,
        format="ICO",
        sizes=[(size, size) for size in ICO_ENTRY_SIZES],
    )


def save_svg(source_path: Path, output_path: Path) -> None:
    output_path.parent.mkdir(parents=True, exist_ok=True)
    _ = shutil.copy2(source_path, output_path)


def sync_targets(
    source: Image.Image, source_path: Path, targets: list[str]
) -> list[Path]:
    written: list[Path] = []
    source_is_svg = source_path.suffix.lower() == ".svg"
    for target in targets:
        config = MANAGED_TARGETS[target]

        for size, relative_path in config.png:
            output_path = ROOT_DIR / relative_path
            save_png(source, size, output_path)
            written.append(output_path)

        for relative_path in config.ico:
            output_path = ROOT_DIR / relative_path
            save_ico(source, output_path)
            written.append(output_path)

        if source_is_svg:
            for relative_path in config.svg:
                output_path = ROOT_DIR / relative_path
                save_svg(source_path, output_path)
                written.append(output_path)

    return written


def main() -> int:
    args = parse_args()
    source_path = resolve_source(args.source)

    try:
        source_image = load_source_image(source_path)
        written = sync_targets(source_image, source_path, args.targets)
    except ValueError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1

    print(f"synced {len(written)} icon files")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
