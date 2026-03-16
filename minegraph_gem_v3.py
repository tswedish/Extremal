#!/usr/bin/env python3
"""
MineGraph Gem Generator v3
--------------------------

Renders graphs as crisp diamond-rotated adjacency matrices with
gem-quality pixel-art styling. The graph's structure IS the art —
edges and non-edges form the pattern, colored by a palette derived
from graph invariants.

Key differences from v2:
- Structure-first: the adjacency matrix diamond is the core visual
- No blur/creature/zone overlays that destroy graph readability
- Color palette derived from graph hash (deterministic)
- Subtle glow effect (like the web app's GraphThumb)
- Clean grid lines and spine
- Each pixel represents a real (i,j) entry in the matrix

Accepted JSON payload:
{
  "bits_b64": "...",
  "encoding": "utri_b64_v1",
  "n": 25
}

Examples
--------
Single render:
    python minegraph_gem_v3.py \\
      --json '{"bits_b64":"AHf4y...","encoding":"utri_b64_v1","n":25}' \\
      --output gem.png

Batch gallery:
    python minegraph_gem_v3.py --batch graphs.jsonl --gallery-dir gems/ --sheet gems/sheet.png
"""

from __future__ import annotations

import argparse
import base64
import colorsys
import hashlib
import json
import math
from dataclasses import dataclass
from pathlib import Path
from typing import List, Optional, Tuple

import numpy as np
from PIL import Image, ImageDraw, ImageFilter


# ── Graph decode ─────────────────────────────────────────────

def decode_utri_b64_v1(bits_b64: str, n: int) -> np.ndarray:
    raw = base64.b64decode(bits_b64)
    total_bits = n * (n - 1) // 2
    needed_bytes = (total_bits + 7) // 8
    if len(raw) < needed_bytes:
        raise ValueError(f"Need {needed_bytes} bytes, got {len(raw)}.")
    bits: list[int] = []
    for byte in raw[:needed_bytes]:
        for k in range(8):
            bits.append((byte >> (7 - k)) & 1)
            if len(bits) >= total_bits:
                break
        if len(bits) >= total_bits:
            break
    A = np.zeros((n, n), dtype=np.uint8)
    t = 0
    for i in range(n):
        for j in range(i + 1, n):
            A[i, j] = bits[t]
            A[j, i] = bits[t]
            t += 1
    return A


def canonical_utri_bytes(A: np.ndarray) -> bytes:
    n = A.shape[0]
    bits: list[int] = []
    for i in range(n):
        for j in range(i + 1, n):
            bits.append(int(A[i, j] != 0))
    out = bytearray()
    cur = 0
    count = 0
    for b in bits:
        cur = (cur << 1) | b
        count += 1
        if count == 8:
            out.append(cur)
            cur = 0
            count = 0
    if count:
        cur <<= (8 - count)
        out.append(cur)
    return bytes(out)


# ── Graph analysis (lightweight) ─────────────────────────────

@dataclass
class GemFeatures:
    n: int
    m: int
    density: float
    degree_std: float
    triangles: int
    graph_hash: str


def triangle_count(A: np.ndarray) -> int:
    B = A.astype(np.int64)
    return int(np.trace(B @ B @ B) // 6)


def analyze(A: np.ndarray) -> GemFeatures:
    n = A.shape[0]
    deg = A.sum(axis=1)
    m = int(deg.sum() // 2)
    max_edges = n * (n - 1) // 2
    packed = canonical_utri_bytes(A)
    return GemFeatures(
        n=n,
        m=m,
        density=m / max_edges if max_edges else 0.0,
        degree_std=float(deg.std()) if n else 0.0,
        triangles=triangle_count(A),
        graph_hash=hashlib.sha256(packed).hexdigest(),
    )


# ── Deterministic palette from graph hash ────────────────────

def hash_floats(seed: bytes, count: int) -> list[float]:
    out: list[float] = []
    cur = seed
    while len(out) < count:
        cur = hashlib.sha256(cur).digest()
        for i in range(0, len(cur), 4):
            if len(out) >= count:
                break
            out.append(int.from_bytes(cur[i : i + 4], "big") / 2**32)
    return out


def hsl(h: float, s: float, l: float) -> Tuple[int, int, int]:
    r, g, b = colorsys.hls_to_rgb(h % 1.0, max(0, min(1, l)), max(0, min(1, s)))
    return (int(round(255 * r)), int(round(255 * g)), int(round(255 * b)))


def make_palette(graph_hash: str) -> dict:
    """Generate a gem palette deterministically from the graph hash."""
    seed = bytes.fromhex(graph_hash)
    v = hash_floats(seed, 16)

    # Base hue from hash
    base_h = v[0]
    # Complementary hue for non-edges
    comp_h = (base_h + 0.45 + 0.10 * v[1]) % 1.0

    # Edge color: saturated, medium-bright
    edge_main = hsl(base_h, 0.70 + 0.20 * v[2], 0.55 + 0.15 * v[3])
    edge_bright = hsl(base_h, 0.60 + 0.20 * v[4], 0.75 + 0.15 * v[5])
    edge_dark = hsl(base_h, 0.50 + 0.15 * v[6], 0.30 + 0.10 * v[7])

    # Non-edge: very dark, slightly tinted
    non_edge = hsl(comp_h, 0.15 + 0.10 * v[8], 0.06 + 0.03 * v[9])

    # Background
    bg = hsl(comp_h, 0.12, 0.03)

    # Spine (diagonal)
    spine = hsl(base_h, 0.25, 0.12 + 0.04 * v[10])

    # Glow color
    glow = hsl(base_h, 0.50 + 0.20 * v[11], 0.40 + 0.10 * v[12])

    # Grid lines
    grid = hsl(base_h, 0.15, 0.10 + 0.04 * v[13])

    # Outline
    outline = hsl(base_h, 0.35, 0.18 + 0.06 * v[14])

    return {
        "edge_main": edge_main,
        "edge_bright": edge_bright,
        "edge_dark": edge_dark,
        "non_edge": non_edge,
        "bg": bg,
        "spine": spine,
        "glow": glow,
        "grid": grid,
        "outline": outline,
    }


# ── Diamond matrix rendering ─────────────────────────────────

def render_gem(A: np.ndarray, features: GemFeatures, cell_size: int = 6) -> Image.Image:
    """
    Render the adjacency matrix as a diamond with gem styling.

    Uses the same coordinate transform as the web app:
      rx = j - i + (n - 1)
      ry = i + j
    """
    n = A.shape[0]
    palette = make_palette(features.graph_hash)
    v = hash_floats(bytes.fromhex(features.graph_hash), 8)

    grid_n = 2 * n - 1
    margin = cell_size * 2
    canvas_size = grid_n * cell_size + 2 * margin

    # Create the sharp diamond image
    img = Image.new("RGB", (canvas_size, canvas_size), palette["bg"])
    draw = ImageDraw.Draw(img)

    # Draw cells
    for i in range(n):
        for j in range(n):
            rx = j - i + (n - 1)
            ry = i + j

            x = margin + rx * cell_size
            y = margin + ry * cell_size

            if i == j:
                color = palette["spine"]
            elif A[i, j]:
                # Edge: vary color slightly by position for texture
                t = (i + j) / (2 * (n - 1)) if n > 1 else 0.5
                if t < 0.33:
                    color = palette["edge_dark"]
                elif t < 0.66:
                    color = palette["edge_main"]
                else:
                    color = palette["edge_bright"]
            else:
                color = palette["non_edge"]

            # Fill cell with slight overlap to prevent gaps
            draw.rectangle(
                [x, y, x + cell_size, y + cell_size],
                fill=color,
            )

    # Draw subtle grid lines
    grid_color = palette["grid"]
    for k in range(grid_n + 1):
        # Horizontal lines (constant i+j)
        x0 = margin
        x1 = margin + grid_n * cell_size
        yy = margin + k * cell_size
        draw.line([(x0, yy), (x1, yy)], fill=grid_color, width=1)

        # Vertical lines (constant j-i)
        y0 = margin
        y1 = margin + grid_n * cell_size
        xx = margin + k * cell_size
        draw.line([(xx, y0), (xx, y1)], fill=grid_color, width=1)

    # Draw diamond outline (the boundary of the n×n matrix in rotated space)
    outline_color = palette["outline"]
    top = (margin + (n - 1) * cell_size, margin)
    right = (margin + (grid_n) * cell_size, margin + (n - 1) * cell_size)
    bottom = (margin + (n - 1) * cell_size, margin + (grid_n) * cell_size)
    left = (margin, margin + (n - 1) * cell_size)
    draw.line([top, right], fill=outline_color, width=2)
    draw.line([right, bottom], fill=outline_color, width=2)
    draw.line([bottom, left], fill=outline_color, width=2)
    draw.line([left, top], fill=outline_color, width=2)

    # Draw spine line (the diagonal, which is the vertical center line)
    spine_x = margin + (n - 1) * cell_size + cell_size // 2
    draw.line(
        [(spine_x, margin), (spine_x, margin + (grid_n) * cell_size)],
        fill=palette["outline"],
        width=1,
    )

    # Mask: only keep the diamond shape, fill outside with bg
    mask = Image.new("L", (canvas_size, canvas_size), 0)
    mask_draw = ImageDraw.Draw(mask)
    # Diamond polygon with small padding
    pad = 2
    mask_draw.polygon(
        [
            (margin + (n - 1) * cell_size + cell_size // 2, margin - pad),
            (margin + grid_n * cell_size + pad, margin + (n - 1) * cell_size + cell_size // 2),
            (margin + (n - 1) * cell_size + cell_size // 2, margin + grid_n * cell_size + pad),
            (margin - pad, margin + (n - 1) * cell_size + cell_size // 2),
        ],
        fill=255,
    )

    bg_img = Image.new("RGB", (canvas_size, canvas_size), palette["bg"])

    # Create glow layer: blur the diamond, tint with glow color
    glow_img = img.copy()
    glow_img = glow_img.filter(ImageFilter.GaussianBlur(radius=cell_size * 1.5))

    # Composite: bg -> glow (soft) -> sharp diamond (masked)
    result = bg_img.copy()

    # Blend glow at low opacity
    result = Image.blend(result, glow_img, alpha=0.35)

    # Paste sharp diamond using the diamond mask
    result.paste(img, mask=mask)

    return result


# ── Sprite sheet ─────────────────────────────────────────────

def add_label(
    img: Image.Image, title: str, subtitle: str, bg: Tuple[int, int, int]
) -> Image.Image:
    band_h = 28
    out = Image.new("RGB", (img.width, img.height + band_h), bg)
    out.paste(img, (0, 0))
    draw = ImageDraw.Draw(out)
    draw.text((6, img.height + 2), title[:32], fill=(210, 215, 230))
    draw.text((6, img.height + 14), subtitle[:40], fill=(140, 150, 170))
    return out


def make_sheet(
    tiles: list[Image.Image],
    columns: int = 5,
    bg: Tuple[int, int, int] = (8, 8, 14),
) -> Image.Image:
    if not tiles:
        raise ValueError("No tiles.")
    cell_w = max(t.width for t in tiles)
    cell_h = max(t.height for t in tiles)
    rows = (len(tiles) + columns - 1) // columns
    gap = 6
    sheet = Image.new(
        "RGB",
        (columns * cell_w + (columns + 1) * gap, rows * cell_h + (rows + 1) * gap),
        bg,
    )
    for idx, tile in enumerate(tiles):
        r = idx // columns
        c = idx % columns
        x = gap + c * (cell_w + gap) + (cell_w - tile.width) // 2
        y = gap + r * (cell_h + gap) + (cell_h - tile.height) // 2
        sheet.paste(tile, (x, y))
    return sheet


# ── I/O ──────────────────────────────────────────────────────

def parse_item(payload: dict, fallback_name: str) -> Tuple[str, np.ndarray]:
    enc = payload.get("encoding")
    if enc != "utri_b64_v1":
        raise ValueError(f"Unsupported encoding {enc!r}")
    bits_b64 = payload.get("bits_b64", "")
    n = payload.get("n", 0)
    name = str(payload.get("name", fallback_name))
    if not bits_b64 or not isinstance(n, int) or n <= 0:
        raise ValueError("Missing bits_b64 or n.")
    return name, decode_utri_b64_v1(bits_b64, n)


def main() -> None:
    ap = argparse.ArgumentParser(description="MineGraph Gem v3 — diamond matrix renderer")
    ap.add_argument("--input", type=str, help="Path to a single JSON graph payload.")
    ap.add_argument("--json", type=str, help="Inline single JSON graph payload.")
    ap.add_argument("--batch", type=str, help="Path to JSONL batch file.")
    ap.add_argument("--output", type=str, default="gem.png", help="Output file (single mode).")
    ap.add_argument("--sheet", type=str, default=None, help="Sprite sheet output path.")
    ap.add_argument("--gallery-dir", type=str, default=None, help="Per-gem output directory.")
    ap.add_argument("--cell-size", type=int, default=6, help="Pixel size per matrix cell.")
    ap.add_argument("--columns", type=int, default=5, help="Gallery columns.")
    args = ap.parse_args()

    cell_size = max(2, args.cell_size)

    if args.batch:
        items: list[Tuple[str, np.ndarray]] = []
        with open(args.batch, "r") as f:
            for idx, line in enumerate(f, 1):
                line = line.strip()
                if not line:
                    continue
                items.append(parse_item(json.loads(line), f"graph_{idx:03d}"))

        if not items:
            raise ValueError("No items in batch file.")

        gallery_dir = Path(args.gallery_dir or "gems")
        gallery_dir.mkdir(parents=True, exist_ok=True)

        tiles: list[Image.Image] = []
        for name, A in items:
            features = analyze(A)
            img = render_gem(A, features, cell_size=cell_size)
            out_path = gallery_dir / f"{name}.png"
            img.save(out_path)

            palette = make_palette(features.graph_hash)
            subtitle = f"n={features.n} m={features.m} d={features.density:.3f} tri={features.triangles}"
            labeled = add_label(img, name, subtitle, palette["bg"])
            tiles.append(labeled)

        sheet_path = Path(args.sheet or (gallery_dir / "sheet.png"))
        sheet = make_sheet(tiles, columns=max(1, args.columns))
        sheet.save(sheet_path)
        print(json.dumps({"count": len(items), "gallery": str(gallery_dir), "sheet": str(sheet_path)}, indent=2))
        return

    # Single mode
    if args.input:
        payload = json.loads(Path(args.input).read_text())
        name, A = parse_item(payload, Path(args.input).stem)
    elif args.json:
        payload = json.loads(args.json)
        name, A = parse_item(payload, "graph")
    else:
        raise ValueError("Provide --input, --json, or --batch.")

    features = analyze(A)
    img = render_gem(A, features, cell_size=cell_size)
    out_path = Path(args.output)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    img.save(out_path)
    print(json.dumps({"name": name, "hash": features.graph_hash, "output": str(out_path)}, indent=2))


if __name__ == "__main__":
    main()
