"""Turn the two generated performance videos into 256px transparent idle clips.

Usage:
  python scripts/build-idle-performances.py --preview <dir>
  python scripts/build-idle-performances.py --out penguin/companion-v4

The source videos use a nearly uniform light background. A border-seeded flood
fill removes it per frame, then frames are premultiplied before downscaling so
the soft edges do not pick up a light halo. Output is static WebP frames
(alpha preserved) because the desktop app already plays frame sequences.
"""
import argparse
import json
import os

import cv2
import numpy as np
from PIL import Image

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SOURCE_DIR = os.path.join(ROOT, "penguin", "companion-v4", "idle-perform-source")
VIDEOS = [
    ("lion", os.path.join(SOURCE_DIR, "lion.mp4")),
    ("holo", os.path.join(SOURCE_DIR, "holo.mp4")),
]
CANVAS = 256
STEP = 2  # 24 fps source -> 12 fps frames
FRAME_MS = 1000.0 * STEP / 24.0
# Keep the full 720px frame. The feet baseline lands at y=236, close enough to
# the built-in idle baseline (243); cropping the source to force 243 would cut
# the feet during the generated close-up shot.
TOP = 0


def save_webp(path, rgb, quality=86):
    Image.fromarray(rgb, "RGBA").save(path, "WEBP", quality=quality, method=4)


def background_mask(bgr, tolerance=8):
    h, w = bgr.shape[:2]
    mask = np.zeros((h + 2, w + 2), np.uint8)
    seeds = []
    for x in range(0, w, 24):
        seeds.append((x, 0))
        seeds.append((x, h - 1))
    for y in range(0, h, 24):
        seeds.append((0, y))
        seeds.append((w - 1, y))
    flags = 4 | (255 << 8) | cv2.FLOODFILL_MASK_ONLY
    for x, y in seeds:
        cv2.floodFill(bgr, mask, (x, y), 0, (tolerance,) * 3, (tolerance,) * 3, flags)
    return mask[1:-1, 1:-1] == 255


def key_frame(frame):
    bg = background_mask(frame.copy())
    alpha = np.where(bg, 0, 255).astype(np.uint8)
    # Generated-video watermarks sit in corners that the character never uses.
    alpha[0:44, 0:160] = 0
    alpha[650:720, 560:720] = 0
    alpha = remove_enclosed_ground(frame, alpha)
    # White costume areas can be connected to the light background by a tiny
    # compression gap. Recover large, color-different regions that touch the
    # existing silhouette, while ignoring isolated background texture.
    hsv = cv2.cvtColor(frame, cv2.COLOR_BGR2HSV)
    candidate = ((hsv[:, :, 1] > 7) | (hsv[:, :, 2] < 210)).astype(np.uint8)
    near_silhouette = cv2.dilate(alpha, np.ones((31, 31), np.uint8), iterations=1)
    count, labels, stats, _ = cv2.connectedComponentsWithStats(candidate, 8)
    for index in range(1, count):
        component = labels == index
        if stats[index, cv2.CC_STAT_AREA] > 300 and (component & (near_silhouette > 0)).any():
            alpha[component] = 255
    # Generated white particles and corner text can be enclosed by glow
    # effects, so border flood fill alone cannot remove them. Keep the main
    # character region protected, then discard near-white low-saturation pixels
    # outside it; cyan hologram pixels remain saturated and are preserved.
    count, labels, stats, _ = cv2.connectedComponentsWithStats(alpha, 8)
    if count > 1:
        main = max(range(1, count), key=lambda index: stats[index, cv2.CC_STAT_AREA])
        x, y, width, height = stats[main, :4]
        protected = np.zeros_like(alpha)
        frame_height, frame_width = frame.shape[:2]
        protected[max(0, y - 50):min(frame_height, y + height + 50), max(0, x - 50):min(frame_width, x + width + 50)] = 1
        hsv = cv2.cvtColor(frame, cv2.COLOR_BGR2HSV)
        white_particle = (hsv[:, :, 1] < 7) & (hsv[:, :, 2] > 210) & (protected == 0)
        alpha[white_particle] = 0
    # The generated close-up has tiny anti-aliased gaps in the dark outline.
    # Flood fill can enter through those gaps and erase the white belly and
    # sleeves in the later hologram frames. Close only small gaps before the
    # final edge cleanup so the full performance keeps the character opaque.
    alpha = cv2.morphologyEx(alpha, cv2.MORPH_CLOSE, np.ones((7, 7), np.uint8), iterations=1)
    # Slight erosion removes the compressed 1px background fringe.
    alpha = cv2.erode(alpha, np.ones((2, 2), np.uint8), iterations=1)
    alpha = cv2.GaussianBlur(alpha, (3, 3), 0)
    rgb = cv2.cvtColor(frame, cv2.COLOR_BGR2RGB)
    return rgb, alpha


def remove_enclosed_ground(frame, alpha):
    """Drop bright background trapped between the feet by the floor shadow.

    The border flood fill cannot reach this patch, so it would otherwise show as
    a white wedge on the desktop. It is a small, bright, neutral component near
    the bottom; the character's belly is much larger and higher up.
    """
    rgb = cv2.cvtColor(frame, cv2.COLOR_BGR2RGB)
    mn = rgb.min(axis=2)
    mx = rgb.max(axis=2)
    bright = ((mn > 224) & ((mx - mn) < 18) & (alpha > 0)).astype(np.uint8)
    count, labels, stats, centroids = cv2.connectedComponentsWithStats(bright, 8)
    height = frame.shape[0]
    for index in range(1, count):
        _, _, box_w, box_h, area = stats[index]
        if 200 <= area <= 8000 and centroids[index][1] > 0.78 * height and box_h < 0.18 * height:
            alpha[labels == index] = 0
    return alpha


def compose_canvas(rgb, alpha):
    a = alpha.astype(np.float32) / 255.0
    prem = rgb.astype(np.float32) * a[..., None]
    prem_small = cv2.resize(prem, (CANVAS, CANVAS), interpolation=cv2.INTER_AREA)
    a_small = cv2.resize(a, (CANVAS, CANVAS), interpolation=cv2.INTER_AREA)
    rgb_small = prem_small / np.maximum(a_small[..., None], 1e-4)
    rgb_small = np.clip(rgb_small, 0, 255).astype(np.uint8)

    # Soften hard canvas cuts (a close-up reaches the bottom/right edge).
    fade = np.ones(CANVAS, np.float32)
    for offset in range(3):
        fade[offset] = fade[CANVAS - 1 - offset] = (offset + 1) / 4.0
    alpha_small = np.clip(a_small * 255.0 * fade[:, None] * fade[None, :], 0, 255).astype(np.uint8)

    out = np.zeros((CANVAS, CANVAS, 4), np.uint8)
    out[TOP:, :, :3] = rgb_small[:CANVAS - TOP]
    out[TOP:, :, 3] = alpha_small[:CANVAS - TOP]
    return out


def build(prefix, source, out_dir):
    cap = cv2.VideoCapture(source)
    if not cap.isOpened():
        raise RuntimeError("cannot open " + source)
    total = int(cap.get(cv2.CAP_PROP_FRAME_COUNT))
    frames = []
    union = [CANVAS, CANVAS, 0, 0]
    for index in range(0, total, STEP):
        cap.set(cv2.CAP_PROP_POS_FRAMES, index)
        ok, frame = cap.read()
        if not ok:
            break
        rgb, alpha = key_frame(frame)
        canvas = compose_canvas(rgb, alpha)
        frames.append((index, canvas))
        ys, xs = np.where(canvas[:, :, 3] > 8)
        if len(xs):
            union[0] = min(union[0], int(xs.min()))
            union[1] = min(union[1], int(ys.min()))
            union[2] = max(union[2], int(xs.max()) + 1)
            union[3] = max(union[3], int(ys.max()) + 1)
    cap.release()

    names = []
    for seq, (_, canvas) in enumerate(frames):
        name = "%s-%03d.webp" % (prefix, seq)
        save_webp(os.path.join(out_dir, name), canvas)
        names.append(name)
    poster = "%s-poster.webp" % prefix
    save_webp(os.path.join(out_dir, poster), frames[0][1], quality=92)

    sizes = [os.path.getsize(os.path.join(out_dir, n)) for n in names]
    meta = {
        "prefix": prefix,
        "source": os.path.basename(source),
        "frames": len(names),
        "frame_ms": FRAME_MS,
        "duration_ms": len(names) * FRAME_MS,
        "union_alpha_bbox": union,
        "total_bytes": sum(sizes),
        "avg_bytes": round(sum(sizes) / max(1, len(sizes))),
    }
    print(json.dumps(meta, ensure_ascii=False))
    return names, meta


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", default="penguin/companion-v4")
    parser.add_argument("--preview", default="")
    args = parser.parse_args()
    out_dir = args.preview or args.out
    os.makedirs(out_dir, exist_ok=True)
    report = {}
    for prefix, source in VIDEOS:
        if not os.path.exists(source):
            raise SystemExit("missing source video: " + source)
        names, meta = build(prefix, source, out_dir)
        report[prefix] = {"names": names, "meta": meta}
    with open(os.path.join(out_dir, "perform-report.json"), "w", encoding="utf-8") as handle:
        json.dump(report, handle, ensure_ascii=False, indent=2)


if __name__ == "__main__":
    main()
