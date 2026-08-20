from __future__ import annotations

from collections import deque
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "branding" / "autotier-icon-b.png"
TAURI = ROOT / "src-tauri" / "icons"
WEB_ICON = ROOT / "src" / "assets" / "icons" / "app-icon.png"


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


def save_png(size: int, path: Path) -> None:
    image = BASE.resize((size, size), Image.Resampling.LANCZOS)
    image.save(path, format="PNG", optimize=True)


BASE = remove_border_white(Image.open(SOURCE))
TAURI.mkdir(parents=True, exist_ok=True)
WEB_ICON.parent.mkdir(parents=True, exist_ok=True)

# Renderer and Tauri canonical assets.
save_png(1024, TAURI / "icon.png")
save_png(1024, WEB_ICON)
save_png(32, TAURI / "32x32.png")
save_png(64, TAURI / "64x64.png")
save_png(128, TAURI / "128x128.png")
save_png(256, TAURI / "128x128@2x.png")

# Windows shell assets used by the existing Tauri/Flatpak packaging templates.
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
    save_png(size, TAURI / filename)

# ICO stores multiple resolutions in one Windows icon.
ico_sizes = [16, 24, 32, 48, 64, 128, 256]
ico_source = BASE.resize((256, 256), Image.Resampling.LANCZOS)
ico_source.save(
    TAURI / "icon.ico",
    format="ICO",
    sizes=[(size, size) for size in ico_sizes],
)

# Keep the source iconset deterministic; convert to ICNS with icnsutil in the shell step.
iconset = ROOT / "branding" / "autotier.iconset"
iconset.mkdir(parents=True, exist_ok=True)
for size in (16, 32, 128, 256, 512, 1024):
    save_png(size, iconset / f"icon_{size}x{size}.png")
    if size < 512:
        save_png(size * 2, iconset / f"icon_{size}x{size}@2x.png")

print(f"Generated AutoTier PNG/ICO/iconset assets from {SOURCE}")
