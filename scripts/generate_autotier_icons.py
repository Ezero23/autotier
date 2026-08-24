from __future__ import annotations

import argparse
from collections import deque
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "branding" / "autotier-icon-b.png"
TAURI = ROOT / "src-tauri" / "icons"
WEB_ICON = ROOT / "src" / "assets" / "icons" / "app-icon.png"
TRAY_MACOS = TAURI / "tray" / "macos"


def remove_border_white(image: Image.Image) -> Image.Image:
    image = image.convert("RGBA")
    pixels = image.load()
    width, height = image.size
    visited: set[tuple[int, int]] = set()
    queue: deque[tuple[int, int]] = deque()

    def is_border_white(x: int, y: int) -> bool:
        r, g, b, a = pixels[x, y]
        return a > 0 and r >= 238 and g >= 238 and b >= 238 and max(r, g, b) - min(r, g, b) <= 12

    for x in range(width):
        if is_border_white(x, 0):
            queue.append((x, 0))
        if is_border_white(x, height - 1):
            queue.append((x, height - 1))
    for y in range(height):
        if is_border_white(0, y):
            queue.append((0, y))
        if is_border_white(width - 1, y):
            queue.append((width - 1, y))

    while queue:
        x, y = queue.popleft()
        if (x, y) in visited or not is_border_white(x, y):
            continue
        visited.add((x, y))
        pixels[x, y] = (255, 255, 255, 0)
        for nx, ny in ((x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)):
            if 0 <= nx < width and 0 <= ny < height and (nx, ny) not in visited:
                queue.append((nx, ny))
    return image


def load_source() -> Image.Image:
    return remove_border_white(Image.open(SOURCE))


def save_png(base: Image.Image, size: int, path: Path) -> None:
    image = base.resize((size, size), Image.Resampling.LANCZOS)
    path.parent.mkdir(parents=True, exist_ok=True)
    image.save(path, format="PNG", optimize=True)


def extract_mark_silhouette(image: Image.Image) -> Image.Image:
    """Keep the cyan/green AutoTier mark as an opaque black template glyph."""
    pixels = image.load()
    width, height = image.size
    out = Image.new("RGBA", image.size, (0, 0, 0, 0))
    dest = out.load()
    for y in range(height):
        for x in range(width):
            r, g, b, a = pixels[x, y]
            if a == 0:
                continue
            if r > 230 and g > 230 and b > 230:
                continue
            lum = 0.2126 * r + 0.7152 * g + 0.0722 * b
            is_navy = lum < 55 and b >= g - 10 and b >= r
            if is_navy:
                continue
            dest[x, y] = (0, 0, 0, 255)
    bbox = out.getbbox()
    if bbox is None:
        raise RuntimeError("failed to extract AutoTier mark from source icon")
    return out.crop(bbox)


def fit_square(image: Image.Image, size: int, pad_ratio: float = 0.14) -> Image.Image:
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    inner = max(1, int(size * (1 - 2 * pad_ratio)))
    fitted = image.copy()
    fitted.thumbnail((inner, inner), Image.Resampling.LANCZOS)
    x = (size - fitted.width) // 2
    y = (size - fitted.height) // 2
    canvas.paste(fitted, (x, y), fitted)
    return canvas


def generate_macos_tray_template(base: Image.Image) -> None:
    """macOS menu-bar template: black glyph, transparent background, 24pt @1x/@2x/@3x."""
    TRAY_MACOS.mkdir(parents=True, exist_ok=True)
    silhouette = extract_mark_silhouette(base)
    sizes = {
        "statusbar_template.png": 24,
        "statusbar_template@2x.png": 48,
        "statusbar_template_3x.png": 72,
    }
    for filename, size in sizes.items():
        icon = fit_square(silhouette, size)
        path = TRAY_MACOS / filename
        icon.save(path, format="PNG", optimize=True)
        print(f"Wrote {path.relative_to(ROOT)} ({size}x{size})")

    for leftover in ("statusTemplate.png", "statusTemplate@2x.png"):
        stale = TRAY_MACOS / leftover
        if stale.exists():
            stale.unlink()
            print(f"Removed leftover CC Switch tray asset {stale.relative_to(ROOT)}")


def generate_app_icons(base: Image.Image) -> None:
    TAURI.mkdir(parents=True, exist_ok=True)
    WEB_ICON.parent.mkdir(parents=True, exist_ok=True)

    save_png(base, 1024, TAURI / "icon.png")
    save_png(base, 1024, WEB_ICON)
    save_png(base, 32, TAURI / "32x32.png")
    save_png(base, 64, TAURI / "64x64.png")
    save_png(base, 128, TAURI / "128x128.png")
    save_png(base, 256, TAURI / "128x128@2x.png")

    for filename, size in {
        "Square30x30Logo.png": 30,
        "Square44x44Logo.png": 44,
        "Square71x71Logo.png": 71,
        "Square89x89Logo.png": 89,
        "Square107x107Logo.png": 107,
        "Square142x142Logo.png": 142,
        "Square150x150Logo.png": 150,
        "Square284x284Logo.png": 284,
        "Square310x310Logo.png": 310,
        "StoreLogo.png": 310,
    }.items():
        save_png(base, size, TAURI / filename)

    ico_sizes = [16, 24, 32, 48, 64, 128, 256]
    ico_source = base.resize((256, 256), Image.Resampling.LANCZOS)
    ico_source.save(
        TAURI / "icon.ico",
        format="ICO",
        sizes=[(size, size) for size in ico_sizes],
    )

    iconset = ROOT / "branding" / "autotier.iconset"
    iconset.mkdir(parents=True, exist_ok=True)
    for size in (16, 32, 128, 256, 512, 1024):
        save_png(base, size, iconset / f"icon_{size}x{size}.png")
        if size < 512:
            save_png(base, size * 2, iconset / f"icon_{size}x{size}@2x.png")


def main() -> None:
    parser = argparse.ArgumentParser(description="Generate AutoTier app and tray icons")
    parser.add_argument(
        "--tray-only",
        action="store_true",
        help="Only regenerate the macOS menu-bar template from the brand mark",
    )
    args = parser.parse_args()
    base = load_source()
    generate_macos_tray_template(base)
    if not args.tray_only:
        generate_app_icons(base)
        print(f"Generated AutoTier PNG/ICO/iconset assets from {SOURCE}")


if __name__ == "__main__":
    main()
