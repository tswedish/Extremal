#!/usr/bin/env python3
"""
MineGraph Gem Generator v2
--------------------------

Deterministic pixel-art generator for graphs encoded as `utri_b64_v1`,
with stronger style families, creature-like rotation, and sprite-sheet/gallery output.

Main additions over v1:
- stronger named style families
- batch input from JSONL
- sprite sheet / contact sheet generation
- creature-oriented rendering: the final gem is rotated and stylized to read
  more like a mask / moth / idol / creature emblem
- optional explicit family selection, or automatic deterministic family choice

Accepted single-item JSON payload:
{
  "bits_b64": "...",
  "encoding": "utri_b64_v1",
  "n": 25
}

Accepted batch JSONL format:
one JSON object per line, each optionally with "name"

Examples
--------
Single render:
    python minegraph_gem_v2.py \
      --json '{"bits_b64":"AHf4yGGvEG04Ytx3TpcZ3QGX9uD7o526XjNETrBkVjlUI5LzouA=","encoding":"utri_b64_v1","n":25,"name":"g1"}' \
      --output gem.png

Single render + metadata:
    python minegraph_gem_v2.py --input graph.json --output gem.png --metadata-json gem_meta.json

Batch gallery:
    python minegraph_gem_v2.py --batch graphs.jsonl --gallery-dir out_gallery --sheet out_gallery/sheet.png

Force a family:
    python minegraph_gem_v2.py --input graph.json --family aurora --output aurora_gem.png
"""

from __future__ import annotations

import argparse
import base64
import colorsys
import hashlib
import json
import math
from dataclasses import dataclass, asdict
from pathlib import Path
from typing import Dict, List, Optional, Tuple

import numpy as np
from PIL import Image, ImageDraw


# ============================================================
# Basic utilities
# ============================================================

def normalize(x: np.ndarray) -> np.ndarray:
    x = x.astype(np.float32)
    mn = float(x.min())
    mx = float(x.max())
    if mx <= mn:
        return np.zeros_like(x, dtype=np.float32)
    return (x - mn) / (mx - mn)


def shift2d(x: np.ndarray, dy: int, dx: int) -> np.ndarray:
    h, w = x.shape
    out = np.zeros_like(x)
    sy0 = max(0, -dy)
    sy1 = min(h, h - dy)
    dy0 = max(0, dy)
    dy1 = min(h, h + dy)

    sx0 = max(0, -dx)
    sx1 = min(w, w - dx)
    dx0 = max(0, dx)
    dx1 = min(w, w + dx)

    out[dy0:dy1, dx0:dx1] = x[sy0:sy1, sx0:sx1]
    return out


def blur3(x: np.ndarray, rounds: int = 1) -> np.ndarray:
    x = x.astype(np.float32)
    for _ in range(rounds):
        acc = (
            x
            + shift2d(x, -1, 0) + shift2d(x, 1, 0)
            + shift2d(x, 0, -1) + shift2d(x, 0, 1)
            + shift2d(x, -1, -1) + shift2d(x, -1, 1)
            + shift2d(x, 1, -1) + shift2d(x, 1, 1)
        )
        x = acc / 9.0
    return x


def edge_mag(x: np.ndarray) -> np.ndarray:
    gx = np.abs(shift2d(x, 0, 1) - shift2d(x, 0, -1))
    gy = np.abs(shift2d(x, 1, 0) - shift2d(x, -1, 0))
    return normalize(gx + gy)


def sigmoid(z: float) -> float:
    return 1.0 / (1.0 + math.exp(-z))


def stable_hash_bytes(data: bytes) -> bytes:
    return hashlib.sha256(data).digest()


def stable_hash_hex(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def hash_floats(seed_bytes: bytes, count: int) -> List[float]:
    out: List[float] = []
    cur = seed_bytes
    while len(out) < count:
        cur = hashlib.sha256(cur).digest()
        for i in range(0, len(cur), 4):
            if len(out) >= count:
                break
            out.append(int.from_bytes(cur[i:i+4], "big") / 2**32)
    return out


def quantize01(v: np.ndarray, levels: int) -> np.ndarray:
    v = np.clip(v, 0.0, 1.0)
    if levels <= 1:
        return np.zeros_like(v)
    return np.round(v * (levels - 1)) / (levels - 1)


def hsl_to_rgb_tuple(h: float, s: float, l: float) -> Tuple[int, int, int]:
    r, g, b = colorsys.hls_to_rgb(h % 1.0, max(0.0, min(1.0, l)), max(0.0, min(1.0, s)))
    return (int(round(255 * r)), int(round(255 * g)), int(round(255 * b)))


def mix_rgb(a: Tuple[int, int, int], b: Tuple[int, int, int], t: float) -> Tuple[int, int, int]:
    t = max(0.0, min(1.0, float(t)))
    return tuple(int(round((1 - t) * aa + t * bb)) for aa, bb in zip(a, b))


def palette_map(score: np.ndarray, palette: List[Tuple[int, int, int]]) -> np.ndarray:
    idx = np.floor(np.clip(score, 0.0, 0.999999) * len(palette)).astype(np.int32)
    idx = np.clip(idx, 0, len(palette) - 1)
    return np.array(palette, dtype=np.uint8)[idx]


# ============================================================
# Graph decode / encode
# ============================================================

def decode_utri_b64_v1(bits_b64: str, n: int) -> np.ndarray:
    raw = base64.b64decode(bits_b64)
    total_bits = n * (n - 1) // 2
    needed_bytes = (total_bits + 7) // 8
    if len(raw) < needed_bytes:
        raise ValueError(f"Payload too short: have {len(raw)} bytes, need {needed_bytes}.")
    bits: List[int] = []
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


def canonical_utri_bits(A: np.ndarray) -> bytes:
    bits: List[int] = []
    n = A.shape[0]
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


# ============================================================
# Graph analysis
# ============================================================

@dataclass
class GraphFeatures:
    name: str
    n: int
    m: int
    density: float
    degree_min: int
    degree_max: int
    degree_mean: float
    degree_std: float
    degree_cv: float
    num_isolates: int
    num_components: int
    largest_component: int
    triangles: int
    triangle_density: float
    complement_triangles: int
    goodman_total: int
    goodman_slack: int
    goodman_balance: float
    spectral_radius: float
    energy: float
    eig_spread: float
    assortativity_proxy: float
    local_clustering_mean: float
    symmetry_score: float
    rarity_score: float
    ornament_level: float
    palette_entropy_target: float
    style_family_auto: str
    graph_hash: str


def count_components(A: np.ndarray) -> Tuple[int, int]:
    n = A.shape[0]
    seen = np.zeros(n, dtype=bool)
    sizes: List[int] = []
    for s in range(n):
        if seen[s]:
            continue
        stack = [s]
        seen[s] = True
        size = 0
        while stack:
            u = stack.pop()
            size += 1
            nbrs = np.flatnonzero(A[u])
            for v in nbrs:
                if not seen[v]:
                    seen[v] = True
                    stack.append(v)
        sizes.append(size)
    return len(sizes), (max(sizes) if sizes else 0)


def triangle_count(A: np.ndarray) -> int:
    B = A.astype(np.int64)
    return int(np.trace(B @ B @ B) // 6)


def complement_adjacency(A: np.ndarray) -> np.ndarray:
    n = A.shape[0]
    return (np.ones_like(A, dtype=np.uint8) - np.eye(n, dtype=np.uint8) - A).astype(np.uint8)


def local_clustering_mean(A: np.ndarray, deg: np.ndarray) -> float:
    n = A.shape[0]
    acc = 0.0
    count = 0
    for i in range(n):
        k = int(deg[i])
        if k < 2:
            continue
        nbrs = np.flatnonzero(A[i])
        sub = A[np.ix_(nbrs, nbrs)]
        e = int(sub.sum() // 2)
        acc += 2.0 * e / (k * (k - 1))
        count += 1
    return acc / count if count else 0.0


def assortativity_proxy(A: np.ndarray, deg: np.ndarray) -> float:
    ii, jj = np.where(np.triu(A, 1))
    if len(ii) < 2:
        return 0.0
    x = deg[ii].astype(np.float64)
    y = deg[jj].astype(np.float64)
    x0 = x - x.mean()
    y0 = y - y.mean()
    xv = float((x0 * x0).mean())
    yv = float((y0 * y0).mean())
    if xv <= 1e-12 or yv <= 1e-12:
        return 0.0
    cov = float((x0 * y0).mean())
    return cov / math.sqrt(xv * yv)


def symmetry_score(A: np.ndarray) -> float:
    n = A.shape[0]
    deg = A.sum(axis=1)
    _, deg_counts = np.unique(deg, return_counts=True)
    degree_repeat = float(np.sum(deg_counts * (deg_counts - 1))) / max(1.0, n * (n - 1))

    row_hashes = [hashlib.sha1(bytes(row.tolist())).digest() for row in A.astype(np.uint8)]
    counts: Dict[bytes, int] = {}
    for h in row_hashes:
        counts[h] = counts.get(h, 0) + 1
    row_repeat = float(sum(c * (c - 1) for c in counts.values())) / max(1.0, n * (n - 1))
    return max(0.0, min(1.0, 0.55 * degree_repeat + 0.45 * row_repeat))


STYLE_FAMILIES = ["ore", "prism", "void", "aurora", "circuit", "cathedral"]


def auto_family_from_features(seed_bytes: bytes, rarity: float, sym: float, density: float, clustering: float) -> str:
    vals = hash_floats(seed_bytes, 6)
    score = (
        1.7 * rarity +
        0.8 * sym +
        0.6 * (1.0 - abs(density - 0.5) / 0.5) +
        0.5 * clustering +
        0.4 * vals[0]
    )
    # Deterministic partitioning with structural bias
    if density < 0.30 and rarity < 0.45:
        return "void"
    if sym > 0.35 and clustering > 0.45:
        return "cathedral"
    if rarity > 0.78:
        return "aurora" if vals[1] > 0.35 else "prism"
    if abs(density - 0.5) < 0.12 and vals[2] > 0.45:
        return "circuit"
    if score < 1.2:
        return "ore"
    return STYLE_FAMILIES[int((vals[3] * 1000) % len(STYLE_FAMILIES))]


def analyze_graph(A: np.ndarray, name: str = "graph") -> GraphFeatures:
    n = A.shape[0]
    deg = A.sum(axis=1).astype(np.int64)
    m = int(deg.sum() // 2)
    max_edges = n * (n - 1) // 2
    density = m / max_edges if max_edges else 0.0

    num_components, largest_component = count_components(A)
    triangles = triangle_count(A)
    comp = complement_adjacency(A)
    comp_triangles = triangle_count(comp)

    goodman_total = math.comb(n, 3) if n >= 3 else 0
    goodman_lower_bound = goodman_total // 4
    goodman_sum = triangles + comp_triangles
    goodman_slack = goodman_sum - goodman_lower_bound
    goodman_balance = 1.0 - goodman_slack / max(1, goodman_total)

    eigvals = np.linalg.eigvalsh(A.astype(np.float64))
    spectral_radius = float(np.max(np.abs(eigvals))) if eigvals.size else 0.0
    energy = float(np.sum(np.abs(eigvals)))
    eig_spread = float(np.std(eigvals)) if eigvals.size else 0.0

    degree_mean = float(deg.mean()) if n else 0.0
    degree_std = float(deg.std()) if n else 0.0
    degree_cv = degree_std / degree_mean if degree_mean > 1e-12 else 0.0
    num_isolates = int(np.sum(deg == 0))
    tri_density = triangles / max(1, goodman_total)
    lcc = local_clustering_mean(A, deg)
    assort = assortativity_proxy(A, deg)
    sym = symmetry_score(A)

    density_centered = 1.0 - min(1.0, abs(density - 0.5) / 0.5)
    regularity_term = math.exp(-4.0 * degree_cv)
    irregularity_term = math.tanh(1.6 * degree_cv)
    connectivity_term = largest_component / max(1, n)
    clustering_surprise = abs(lcc - density)
    spectral_term = sigmoid((spectral_radius - degree_mean) / max(1.0, math.sqrt(n)))

    rarity = (
        0.31 * max(0.0, min(1.0, goodman_balance)) +
        0.15 * max(regularity_term, 0.75 * irregularity_term) +
        0.11 * density_centered +
        0.10 * connectivity_term +
        0.12 * clustering_surprise +
        0.08 * spectral_term +
        0.13 * sym
    )
    rarity = max(0.0, min(1.0, rarity))
    ornament = max(0.0, min(1.0, 0.15 + 0.85 * rarity))
    palette_entropy_target = max(0.0, min(1.0, 0.12 + 0.82 * rarity + 0.10 * sym))

    packed = canonical_utri_bits(A)
    graph_hash = stable_hash_hex(packed)
    seed_bytes = stable_hash_bytes(packed)
    auto_family = auto_family_from_features(seed_bytes, rarity, sym, density, lcc)

    return GraphFeatures(
        name=name,
        n=n,
        m=m,
        density=density,
        degree_min=int(deg.min()) if n else 0,
        degree_max=int(deg.max()) if n else 0,
        degree_mean=degree_mean,
        degree_std=degree_std,
        degree_cv=degree_cv,
        num_isolates=num_isolates,
        num_components=num_components,
        largest_component=largest_component,
        triangles=triangles,
        triangle_density=tri_density,
        complement_triangles=comp_triangles,
        goodman_total=goodman_total,
        goodman_slack=goodman_slack,
        goodman_balance=goodman_balance,
        spectral_radius=spectral_radius,
        energy=energy,
        eig_spread=eig_spread,
        assortativity_proxy=assort,
        local_clustering_mean=lcc,
        symmetry_score=sym,
        rarity_score=rarity,
        ornament_level=ornament,
        palette_entropy_target=palette_entropy_target,
        style_family_auto=auto_family,
        graph_hash=graph_hash,
    )


# ============================================================
# Palettes by family
# ============================================================

def family_palette(features: GraphFeatures, family: str, seed_bytes: bytes) -> Tuple[List[Tuple[int, int, int]], Tuple[int, int, int], Tuple[int, int, int]]:
    vals = hash_floats(seed_bytes, 32)
    density = features.density
    rarity = features.rarity_score
    sym = features.symmetry_score
    balance = features.goodman_balance

    base_h = (
        0.21 * vals[0] +
        0.17 * density +
        0.11 * sym +
        0.14 * (1.0 - balance) +
        0.07 * (0.5 + 0.5 * features.assortativity_proxy)
    ) % 1.0

    if family == "ore":
        h = (base_h + 0.08 * vals[1]) % 1.0
        metal = hsl_to_rgb_tuple(h, 0.18 + 0.12 * vals[2], 0.78)
        mineral = hsl_to_rgb_tuple((h + 0.05) % 1.0, 0.35 + 0.20 * rarity, 0.42)
        shadow = hsl_to_rgb_tuple(h, 0.18, 0.06)
        palette = [
            shadow,
            hsl_to_rgb_tuple(h, 0.14, 0.10),
            hsl_to_rgb_tuple(h, 0.18, 0.18),
            hsl_to_rgb_tuple((h + 0.02) % 1.0, 0.22, 0.28),
            mineral,
            hsl_to_rgb_tuple((h + 0.03) % 1.0, 0.30 + 0.15 * rarity, 0.58),
            metal,
            hsl_to_rgb_tuple((h + 0.04) % 1.0, 0.18 + 0.05 * rarity, 0.90),
        ]
        bg = hsl_to_rgb_tuple((h + 0.06) % 1.0, 0.20, 0.04)
        aura = hsl_to_rgb_tuple((h + 0.02) % 1.0, 0.22, 0.16)
    elif family == "prism":
        span = 0.34 + 0.18 * rarity
        palette = []
        for i in range(8):
            t = i / 7.0
            h = (base_h + span * t) % 1.0
            s = 0.62 + 0.22 * rarity
            l = 0.06 + 0.84 * (t ** 0.92)
            palette.append(hsl_to_rgb_tuple(h, s, l))
        bg = hsl_to_rgb_tuple((base_h + 0.62) % 1.0, 0.35, 0.04)
        aura = hsl_to_rgb_tuple((base_h + 0.14) % 1.0, 0.52, 0.18)
    elif family == "void":
        h = (base_h + 0.76 + 0.08 * vals[3]) % 1.0
        accent = (h + 0.12 + 0.10 * vals[4]) % 1.0
        palette = [
            (0, 0, 0),
            hsl_to_rgb_tuple(h, 0.25, 0.05),
            hsl_to_rgb_tuple(h, 0.45, 0.10),
            hsl_to_rgb_tuple(h, 0.70, 0.18),
            hsl_to_rgb_tuple(accent, 0.80, 0.34),
            hsl_to_rgb_tuple(accent, 0.88, 0.54),
            hsl_to_rgb_tuple(accent, 0.60, 0.72),
            hsl_to_rgb_tuple(accent, 0.40, 0.90),
        ]
        bg = hsl_to_rgb_tuple(h, 0.55, 0.02)
        aura = hsl_to_rgb_tuple(accent, 0.80, 0.16)
    elif family == "aurora":
        span = 0.62 + 0.22 * rarity
        palette = []
        for i in range(8):
            t = i / 7.0
            h = (base_h + span * t + 0.03 * math.sin(2 * math.pi * (t + vals[5]))) % 1.0
            s = 0.74 + 0.20 * rarity
            l = 0.05 + 0.85 * (t ** 0.95)
            palette.append(hsl_to_rgb_tuple(h, s, l))
        bg = hsl_to_rgb_tuple((base_h + 0.40) % 1.0, 0.45, 0.035)
        aura = hsl_to_rgb_tuple((base_h + 0.18) % 1.0, 0.60, 0.16)
    elif family == "circuit":
        h = (base_h + 0.28 * vals[6]) % 1.0
        neon = (h + 0.48 + 0.08 * vals[7]) % 1.0
        palette = [
            hsl_to_rgb_tuple(h, 0.30, 0.03),
            hsl_to_rgb_tuple(h, 0.35, 0.08),
            hsl_to_rgb_tuple(h, 0.45, 0.16),
            hsl_to_rgb_tuple(h, 0.52, 0.28),
            hsl_to_rgb_tuple(neon, 0.84, 0.42),
            hsl_to_rgb_tuple(neon, 0.92, 0.58),
            hsl_to_rgb_tuple(neon, 0.62, 0.76),
            hsl_to_rgb_tuple(neon, 0.38, 0.92),
        ]
        bg = hsl_to_rgb_tuple(h, 0.35, 0.03)
        aura = hsl_to_rgb_tuple(neon, 0.72, 0.15)
    elif family == "cathedral":
        h1 = (base_h + 0.03) % 1.0
        h2 = (base_h + 0.56 + 0.06 * vals[8]) % 1.0
        palette = [
            hsl_to_rgb_tuple(h1, 0.35, 0.05),
            hsl_to_rgb_tuple(h1, 0.42, 0.12),
            hsl_to_rgb_tuple(h1, 0.55, 0.22),
            hsl_to_rgb_tuple(h2, 0.64, 0.32),
            hsl_to_rgb_tuple(h2, 0.72, 0.48),
            hsl_to_rgb_tuple(h2, 0.72, 0.62),
            hsl_to_rgb_tuple((h2 + 0.06) % 1.0, 0.54, 0.80),
            hsl_to_rgb_tuple((h2 + 0.02) % 1.0, 0.32, 0.94),
        ]
        bg = hsl_to_rgb_tuple(h1, 0.26, 0.035)
        aura = hsl_to_rgb_tuple(h2, 0.42, 0.16)
    else:
        raise ValueError(f"Unknown family {family!r}")

    return palette, bg, aura


# ============================================================
# Raster construction
# ============================================================

def diamond_embed(mat: np.ndarray, scale: int = 2) -> np.ndarray:
    n, m = mat.shape
    H = (n + m - 1) * scale + 5
    W = (n + m - 1) * scale + 5
    out = np.zeros((H, W), dtype=np.float32)
    for i in range(n):
        for j in range(m):
            y = 2 + ((i + j) * scale) // 2
            x = 2 + W // 2 + ((j - i) * scale) // 2
            if 0 <= y < H and 0 <= x < W:
                out[y, x] = max(out[y, x], float(mat[i, j]))
    return out


def diamond_expand(points: np.ndarray, radius: int = 1) -> np.ndarray:
    out = points.copy()
    for r in range(1, radius + 1):
        out = np.maximum(out, shift2d(points, -r, 0))
        out = np.maximum(out, shift2d(points, 1 * r, 0))
        out = np.maximum(out, shift2d(points, 0, -r))
        out = np.maximum(out, shift2d(points, 0, 1 * r))
    return out


def build_creature_fields(A: np.ndarray, features: GraphFeatures) -> Dict[str, np.ndarray]:
    """
    Build a richer field stack from the graph, with explicit creature / crest bias.

    The idea:
    - start from the diamond-embedded adjacency bits
    - add a bilateral "face axis"
    - encourage eye/crest/wing motifs from deterministic graph features
    """
    base_pts = diamond_embed(A.astype(np.float32), scale=2)
    base = diamond_expand(base_pts, radius=1)

    h, w = base.shape
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float32)
    cy, cx = (h - 1) / 2.0, (w - 1) / 2.0
    rx = (xx - cx) / max(1.0, cx)
    ry = (yy - cy) / max(1.0, cy)
    r = np.sqrt(rx * rx + ry * ry)
    theta = np.arctan2(ry, rx)

    density1 = blur3(base, 1)
    density2 = blur3(base, 3)
    density3 = blur3(base, 7)
    density4 = blur3(base, 12)

    edge = edge_mag(density1)
    glow = normalize(blur3(base, 10))
    soft = normalize(blur3(base, 18))

    diamond_mask = np.clip(1.0 - 0.96 * (np.abs(rx) + np.abs(ry)), 0.0, 1.0)
    diamond_mask = np.power(normalize(diamond_mask), 0.95)

    # Bilateral mask for creature-like reading
    bilateral = 1.0 - np.abs(rx)
    bilateral = np.clip(bilateral, 0.0, 1.0)

    # Head / thorax / wing zones
    head_zone = np.exp(-(((rx / 0.18) ** 2) + (((ry + 0.38) / 0.11) ** 2)))
    eye_zone_l = np.exp(-((((rx + 0.16) / 0.07) ** 2) + (((ry + 0.20) / 0.06) ** 2)))
    eye_zone_r = np.exp(-((((rx - 0.16) / 0.07) ** 2) + (((ry + 0.20) / 0.06) ** 2)))
    thorax_zone = np.exp(-(((rx / 0.20) ** 2) + (((ry + 0.02) / 0.16) ** 2)))
    wing_zone = np.exp(-((((np.abs(rx) - 0.42) / 0.20) ** 2) + (((ry + 0.02) / 0.25) ** 2)))
    tail_zone = np.exp(-(((rx / 0.14) ** 2) + (((ry - 0.42) / 0.16) ** 2)))

    # Faceting / ornament fields
    facet = np.maximum.reduce([
        np.abs((xx + yy) % 6 - 3),
        np.abs((xx - yy) % 8 - 4),
        np.abs((2 * xx + yy) % 10 - 5),
    ]).astype(np.float32)
    facet = 1.0 - normalize(facet)

    stripes = 0.5 + 0.5 * np.cos(10 * theta - 7 * r)
    rings = 0.5 + 0.5 * np.cos((11 + 9 * features.ornament_level) * r + 2.1 * density2)
    petals = 0.5 + 0.5 * np.cos((6 + int(8 * features.symmetry_score)) * theta + 0.5 * np.sin(4 * theta))
    checker = (((xx.astype(np.int32) ^ yy.astype(np.int32)) & 1).astype(np.float32))

    return {
        "base": base,
        "density1": density1,
        "density2": density2,
        "density3": density3,
        "density4": density4,
        "edge": edge,
        "glow": glow,
        "soft": soft,
        "diamond_mask": diamond_mask,
        "bilateral": bilateral,
        "head_zone": head_zone,
        "eye_zone_l": eye_zone_l,
        "eye_zone_r": eye_zone_r,
        "thorax_zone": thorax_zone,
        "wing_zone": wing_zone,
        "tail_zone": tail_zone,
        "facet": facet,
        "stripes": stripes,
        "rings": rings,
        "petals": petals,
        "checker": checker,
        "r": r,
        "theta": theta,
        "rx": rx,
        "ry": ry,
    }


def score_field_for_family(fields: Dict[str, np.ndarray], features: GraphFeatures, family: str, seed_bytes: bytes) -> np.ndarray:
    vals = hash_floats(seed_bytes, 24)
    rarity = features.rarity_score
    orn = features.ornament_level
    sym = features.symmetry_score
    density = features.density

    base = fields["base"]
    density2 = fields["density2"]
    density3 = fields["density3"]
    density4 = fields["density4"]
    edge = fields["edge"]
    glow = fields["glow"]
    soft = fields["soft"]
    mask = fields["diamond_mask"]
    bilateral = fields["bilateral"]
    head = fields["head_zone"]
    eye_l = fields["eye_zone_l"]
    eye_r = fields["eye_zone_r"]
    thorax = fields["thorax_zone"]
    wing = fields["wing_zone"]
    tail = fields["tail_zone"]
    facet = fields["facet"]
    stripes = fields["stripes"]
    rings = fields["rings"]
    petals = fields["petals"]
    checker = fields["checker"]
    r = fields["r"]
    rx = fields["rx"]
    ry = fields["ry"]

    # Shared creature scaffold
    shared = (
        0.90 * density4 +
        0.38 * glow +
        0.18 * soft +
        0.22 * edge +
        0.13 * facet * soft +
        0.12 * petals * density3 * (0.35 + 0.65 * sym) +
        0.16 * rings * density3 * orn +
        0.08 * checker * density2 +
        0.12 * bilateral * (0.5 * thorax + 0.3 * wing + 0.2 * head) +
        0.18 * base
    )

    # Eye/crest emphasis. Deterministic, not always strong.
    eye_boost = (0.05 + 0.18 * rarity) * (eye_l + eye_r) * (0.4 + 0.6 * edge)
    crest = np.exp(-(((rx / (0.08 + 0.03 * vals[0])) ** 2) + (((ry + 0.48) / (0.10 + 0.03 * vals[1])) ** 2)))
    shared += eye_boost + 0.10 * crest * glow

    if family == "ore":
        score = (
            shared +
            0.18 * facet * glow +
            0.10 * stripes * density2 +
            0.11 * wing * density2 +
            0.12 * tail * edge
        )
        score -= 0.18 * normalize(np.abs(density2 - density4))
    elif family == "prism":
        score = (
            shared +
            0.22 * stripes * glow +
            0.14 * wing * rings +
            0.10 * head * glow +
            0.10 * (1.0 - np.abs(rx)) * edge
        )
        score += 0.08 * np.exp(-((r / 0.16) ** 2))
    elif family == "void":
        void_core = np.exp(-((r / (0.17 + 0.03 * vals[2])) ** 2))
        score = (
            0.82 * shared +
            0.24 * edge +
            0.20 * glow * wing +
            0.16 * petals * density2
        )
        score -= (0.16 + 0.25 * (1.0 - density)) * void_core
        score += 0.14 * (eye_l + eye_r) * glow
    elif family == "aurora":
        score = (
            shared +
            0.22 * stripes * glow +
            0.18 * rings * soft +
            0.16 * wing * petals +
            0.10 * head * stripes
        )
        score += 0.05 * np.sin(18 * ry + 6 * rx) * mask
    elif family == "circuit":
        grid = 1.0 - normalize(
            np.minimum.reduce([
                np.abs((fields["rx"] * 80).astype(np.int32) % 6 - 3),
                np.abs((fields["ry"] * 80).astype(np.int32) % 6 - 3)
            ]).astype(np.float32)
        )
        score = (
            0.86 * shared +
            0.22 * grid * glow +
            0.18 * edge * thorax +
            0.14 * wing * checker +
            0.12 * tail * grid
        )
        score += 0.10 * (eye_l + eye_r) * (0.5 + 0.5 * grid)
    elif family == "cathedral":
        spine = np.exp(-((rx / (0.08 + 0.02 * vals[3])) ** 2))
        arch = np.exp(-((((np.abs(rx) - 0.32) / 0.13) ** 2) + (((ry + 0.05) / 0.23) ** 2)))
        score = (
            0.90 * shared +
            0.18 * spine * glow +
            0.16 * arch * soft +
            0.15 * petals * thorax +
            0.10 * head * edge
        )
        score += 0.08 * (eye_l + eye_r) * edge
    else:
        raise ValueError(f"Unknown family {family!r}")

    # Outline trenches preserve pixel structure
    trenches = normalize(np.abs(fields["density1"] - fields["density3"]) + 0.5 * np.abs(fields["density2"] - fields["density4"]))
    score -= (0.10 + 0.18 * vals[4]) * trenches

    # Shape concentration
    score *= (0.24 + 0.76 * mask)

    # Peripheral aura
    aura = np.exp(-((np.maximum(0.0, r - 0.63) / (0.10 + 0.04 * vals[5])) ** 2))
    score += 0.05 * aura * (0.35 + 0.65 * rarity)

    score = normalize(score)
    gamma = 0.72 + 0.45 * (1.0 - rarity) + 0.15 * vals[6]
    score = np.power(score, gamma)

    levels = 6 + int(round(2.0 * rarity))
    score = quantize01(score, levels)
    return score


def render_gem_image(A: np.ndarray, features: GraphFeatures, family: str, upscale: int = 8) -> Image.Image:
    packed = canonical_utri_bits(A)
    seed_bytes = stable_hash_bytes(packed + family.encode("utf-8"))
    palette, bg, aura = family_palette(features, family, seed_bytes)
    fields = build_creature_fields(A, features)
    score = score_field_for_family(fields, features, family, seed_bytes)

    rgb = palette_map(score, palette)
    edge = fields["edge"]
    mask = fields["diamond_mask"]
    glow = fields["glow"]
    eye_l = fields["eye_zone_l"]
    eye_r = fields["eye_zone_r"]

    # Background fill
    bg_arr = np.zeros_like(rgb) + np.array(bg, dtype=np.uint8)[None, None, :]
    rgb = np.where(mask[..., None] > 0.01, rgb, bg_arr)

    # Outline
    outline = (edge > 0.56) & (mask > 0.02)
    rgb[outline] = np.maximum(rgb[outline] // 3, 0)

    # Aura border
    border = (mask > 0.0) & (mask < 0.08)
    aura_arr = np.array(aura, dtype=np.uint8)
    rgb[border] = np.clip(0.68 * rgb[border] + 0.32 * aura_arr, 0, 255).astype(np.uint8)

    # Deterministic sparkle / eyes
    rarity = features.rarity_score
    if rarity > 0.48:
        eye_mask = ((eye_l + eye_r) > 0.46) & (glow > 0.25)
        rgb[eye_mask] = np.clip(rgb[eye_mask].astype(np.int16) + 50, 0, 255).astype(np.uint8)

    # Upscale first, then rotate so the whole emblem reads as a creature crest / idol
    img = Image.fromarray(rgb, mode="RGB")
    img = img.resize((img.width * upscale, img.height * upscale), resample=Image.Resampling.NEAREST)

    # Creature orientation: rotate 45 degrees into a more emblematic, face-like stance
    img = img.rotate(
        45,
        resample=Image.Resampling.NEAREST,
        expand=True,
        fillcolor=bg
    )

    # Crop to content with margin
    arr = np.array(img)
    diff = np.any(arr != np.array(bg, dtype=np.uint8)[None, None, :], axis=2)
    ys, xs = np.where(diff)
    if len(xs) > 0 and len(ys) > 0:
        x0 = max(0, int(xs.min()) - 8)
        x1 = min(img.width, int(xs.max()) + 9)
        y0 = max(0, int(ys.min()) - 8)
        y1 = min(img.height, int(ys.max()) + 9)
        img = img.crop((x0, y0, x1, y1))
    return img


# ============================================================
# Gallery / sprite sheet
# ============================================================

def add_label_band(img: Image.Image, title: str, subtitle: str, bg: Tuple[int, int, int]) -> Image.Image:
    band_h = 26
    out = Image.new("RGB", (img.width, img.height + band_h), bg)
    out.paste(img, (0, 0))
    draw = ImageDraw.Draw(out)
    text_color = (230, 235, 245)
    sub_color = (165, 175, 195)

    # Built-in bitmap font; deterministic and dependency-free.
    draw.text((4, img.height + 2), title[:28], fill=text_color)
    draw.text((4, img.height + 13), subtitle[:36], fill=sub_color)
    return out


def make_sheet(tiles: List[Image.Image], columns: int = 4, sheet_bg: Tuple[int, int, int] = (10, 12, 18)) -> Image.Image:
    if not tiles:
        raise ValueError("No tiles provided.")
    cell_w = max(t.width for t in tiles)
    cell_h = max(t.height for t in tiles)
    rows = (len(tiles) + columns - 1) // columns
    margin = 8
    sheet = Image.new(
        "RGB",
        (columns * cell_w + (columns + 1) * margin, rows * cell_h + (rows + 1) * margin),
        sheet_bg
    )
    for idx, tile in enumerate(tiles):
        r = idx // columns
        c = idx % columns
        x = margin + c * (cell_w + margin) + (cell_w - tile.width) // 2
        y = margin + r * (cell_h + margin) + (cell_h - tile.height) // 2
        sheet.paste(tile, (x, y))
    return sheet


# ============================================================
# I/O helpers
# ============================================================

def parse_item(payload: Dict, fallback_name: str) -> Tuple[str, np.ndarray]:
    enc = payload.get("encoding")
    if enc != "utri_b64_v1":
        raise ValueError(f"Unsupported encoding {enc!r}; expected 'utri_b64_v1'.")
    bits_b64 = payload.get("bits_b64")
    n = payload.get("n")
    name = str(payload.get("name", fallback_name))
    if not isinstance(bits_b64, str) or not bits_b64:
        raise ValueError("Missing valid bits_b64.")
    if not isinstance(n, int) or n <= 0:
        raise ValueError("Missing valid n.")
    A = decode_utri_b64_v1(bits_b64, n)
    return name, A


def load_single(input_path: Optional[str], inline_json: Optional[str]) -> Tuple[str, np.ndarray]:
    if input_path:
        payload = json.loads(Path(input_path).read_text())
        return parse_item(payload, Path(input_path).stem)
    if inline_json:
        payload = json.loads(inline_json)
        return parse_item(payload, "graph")
    raise ValueError("Provide either --input or --json.")


def load_batch(batch_path: str) -> List[Tuple[str, np.ndarray]]:
    items: List[Tuple[str, np.ndarray]] = []
    with Path(batch_path).open("r", encoding="utf-8") as f:
        for idx, line in enumerate(f, start=1):
            line = line.strip()
            if not line:
                continue
            payload = json.loads(line)
            items.append(parse_item(payload, f"graph_{idx:03d}"))
    return items


# ============================================================
# CLI
# ============================================================

def main() -> None:
    ap = argparse.ArgumentParser(description="MineGraph Gem v2 generator.")
    ap.add_argument("--input", type=str, help="Path to a single JSON graph payload.")
    ap.add_argument("--json", type=str, help="Inline single JSON graph payload.")
    ap.add_argument("--batch", type=str, help="Path to JSONL batch file.")
    ap.add_argument("--output", type=str, default="minegraph_gem_v2.png", help="Output file for single render.")
    ap.add_argument("--sheet", type=str, default=None, help="Sprite sheet / gallery output path.")
    ap.add_argument("--gallery-dir", type=str, default=None, help="Directory for per-gem outputs in batch mode.")
    ap.add_argument("--metadata-json", type=str, default=None, help="Write metadata JSON for single render.")
    ap.add_argument("--family", type=str, default="auto", choices=["auto"] + STYLE_FAMILIES, help="Family override.")
    ap.add_argument("--upscale", type=int, default=8, help="Nearest-neighbor upscale.")
    ap.add_argument("--columns", type=int, default=4, help="Gallery columns.")
    args = ap.parse_args()

    if args.batch:
        items = load_batch(args.batch)
        if not items:
            raise ValueError("Batch file contained no items.")

        gallery_dir = Path(args.gallery_dir or "minegraph_gallery")
        gallery_dir.mkdir(parents=True, exist_ok=True)

        tiles: List[Image.Image] = []
        metadata_rows: List[Dict] = []

        for name, A in items:
            features = analyze_graph(A, name=name)
            family = features.style_family_auto if args.family == "auto" else args.family
            img = render_gem_image(A, features, family=family, upscale=max(1, int(args.upscale)))

            out_path = gallery_dir / f"{name}_{family}.png"
            img.save(out_path)

            title = name[:22]
            subtitle = f"{family} | r={features.rarity_score:.3f}"
            tile_bg = (8, 10, 18)
            labeled = add_label_band(img, title, subtitle, tile_bg)
            tiles.append(labeled)

            row = asdict(features)
            row["family"] = family
            row["output"] = str(out_path)
            metadata_rows.append(row)

        sheet_path = Path(args.sheet or (gallery_dir / "sheet.png"))
        sheet = make_sheet(tiles, columns=max(1, int(args.columns)), sheet_bg=(6, 8, 14))
        sheet.save(sheet_path)

        meta_path = gallery_dir / "gallery_metadata.json"
        meta_path.write_text(json.dumps(metadata_rows, indent=2))
        print(json.dumps({
            "count": len(items),
            "gallery_dir": str(gallery_dir),
            "sheet": str(sheet_path),
            "metadata": str(meta_path),
        }, indent=2))
        return

    name, A = load_single(args.input, args.json)
    features = analyze_graph(A, name=name)
    family = features.style_family_auto if args.family == "auto" else args.family
    img = render_gem_image(A, features, family=family, upscale=max(1, int(args.upscale)))

    out_path = Path(args.output)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    img.save(out_path)

    meta = asdict(features)
    meta["family"] = family
    meta["output"] = str(out_path)
    print(json.dumps(meta, indent=2))

    if args.metadata_json:
        Path(args.metadata_json).write_text(json.dumps(meta, indent=2))


if __name__ == "__main__":
    main()
