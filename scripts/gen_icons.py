"""Generate Tauri + NSIS icons from peach-cat.gif (first frame)."""
from __future__ import annotations

import struct
from io import BytesIO
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "ui" / "assets" / "peach-cat.gif"
OUT = ROOT / "src-tauri" / "icons"

# Match app background
BG = (7, 6, 15, 255)  # #07060f


def square_fit(im: Image.Image, size: int, bg: tuple[int, int, int, int] | None = None) -> Image.Image:
    w, h = im.size
    scale = min(size / w, size / h)
    nw = max(1, int(round(w * scale)))
    nh = max(1, int(round(h * scale)))
    resized = im.resize((nw, nh), Image.Resampling.LANCZOS)
    canvas = Image.new("RGBA", (size, size), bg if bg is not None else (0, 0, 0, 0))
    canvas.paste(resized, ((size - nw) // 2, (size - nh) // 2), resized)
    return canvas


def fit_into(im: Image.Image, box: tuple[int, int], pad: float = 0.82) -> Image.Image:
    """Center peach cat on a solid dark canvas of box size (NSIS BMPs need opaque RGB)."""
    bw, bh = box
    canvas = Image.new("RGBA", (bw, bh), BG)
    max_w = int(bw * pad)
    max_h = int(bh * pad)
    w, h = im.size
    scale = min(max_w / w, max_h / h)
    nw = max(1, int(round(w * scale)))
    nh = max(1, int(round(h * scale)))
    resized = im.resize((nw, nh), Image.Resampling.LANCZOS)
    canvas.paste(resized, ((bw - nw) // 2, (bh - nh) // 2), resized)
    return canvas.convert("RGB")


def write_ico(frame: Image.Image, path: Path) -> None:
    ico_sizes = [16, 32, 48, 64, 128, 256]
    entries: list[tuple[int, bytes]] = []
    for s in ico_sizes:
        buf = BytesIO()
        # Opaque background so installer / shell icons look clean on Windows
        square_fit(frame, s, BG).save(buf, format="PNG")
        data = buf.getvalue()
        entries.append((s, data))

    num = len(entries)
    header = struct.pack("<HHH", 0, 1, num)
    offset = 6 + 16 * num
    dir_entries = b""
    image_data = b""
    for s, data in entries:
        w = 0 if s >= 256 else s
        h = 0 if s >= 256 else s
        dir_entries += struct.pack("<BBBBHHII", w, h, 0, 0, 1, 32, len(data), offset)
        image_data += data
        offset += len(data)

    path.write_bytes(header + dir_entries + image_data)
    print(f"{path.name}: {path.stat().st_size}B")


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    img = Image.open(SRC)
    img.seek(0)
    frame = img.convert("RGBA")

    for name, size in [
        ("32x32.png", 32),
        ("128x128.png", 128),
        ("128x128@2x.png", 256),
        ("icon.png", 512),
    ]:
        p = square_fit(frame, size, BG)
        dest = OUT / name
        p.save(dest, "PNG")
        print(f"{name}: {dest.stat().st_size}B {p.size}")

    write_ico(frame, OUT / "icon.ico")

    # NSIS installer chrome (recommended sizes from Tauri docs)
    header = fit_into(frame, (150, 57), pad=0.9)
    header.save(OUT / "nsis-header.bmp", format="BMP")
    print(f"nsis-header.bmp: {(OUT / 'nsis-header.bmp').stat().st_size}B")

    sidebar = fit_into(frame, (164, 314), pad=0.55)
    sidebar.save(OUT / "nsis-sidebar.bmp", format="BMP")
    print(f"nsis-sidebar.bmp: {(OUT / 'nsis-sidebar.bmp').stat().st_size}B")


if __name__ == "__main__":
    main()
