"""Build a continuous idle loop from original forward and reverse video frames.

Usage: python prepare-idle-loop.py source.webm ffmpeg.exe source-pack output-dir
No model, rescaling or image synthesis is used. FFmpeg is an offline tool only.
"""
import argparse
import hashlib
import json
import subprocess
from pathlib import Path

import numpy as np


def run(args, data=None):
    result = subprocess.run(
        [str(arg) for arg in args], input=data, capture_output=True,
        timeout=240, check=False,
    )
    if result.returncode:
        raise RuntimeError(result.stderr.decode("utf-8", errors="replace")[-2400:])
    return result.stdout


def decode(ffmpeg, path):
    raw = run([
        ffmpeg, "-hide_banner", "-loglevel", "error", "-nostdin",
        "-c:v", "libvpx-vp9", "-i", path, "-an",
        "-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1",
    ])
    return np.frombuffer(raw, dtype=np.uint8).reshape(-1, 256, 256, 4)


def metrics(frames):
    alpha = frames[..., 3:4].astype(np.float32) / 255
    visible = frames[..., :3].astype(np.float32) * alpha + 190 * (1 - alpha)
    steps = np.abs(visible[1:] - visible[:-1]).mean(axis=(1, 2, 3))
    return {
        "frameCount": len(frames),
        "internalMedianDelta": float(np.median(steps)),
        "internalP95Delta": float(np.quantile(steps, .95)),
        "internalMaxDelta": float(steps.max()),
        "seamDelta": float(np.abs(visible[-1] - visible[0]).mean()),
        "cornerAlphaMax": int(frames[:, :8, :8, 3].max()),
    }


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("ffmpeg", type=Path)
    parser.add_argument("source_pack", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    if args.source.resolve().parent == args.output.resolve():
        raise ValueError("Use a separate output directory to preserve the source")
    args.output.mkdir(parents=True, exist_ok=True)
    source = decode(args.ffmpeg, args.source)
    if len(source) < 3:
        raise ValueError("At least three video frames are required")
    # Avoid duplicate endpoints: 0..N-1, N-2..1, then the next cycle starts at 0.
    body = np.concatenate([source, source[-2:0:-1]], axis=0)
    fps = 24
    video = args.output / "idle.webm"
    run([
        args.ffmpeg, "-hide_banner", "-loglevel", "error", "-nostdin", "-y",
        "-f", "rawvideo", "-pix_fmt", "rgba", "-s", "256x256",
        "-framerate", fps, "-i", "pipe:0", "-an", "-c:v", "libvpx-vp9",
        "-pix_fmt", "yuva420p", "-auto-alt-ref", "0", "-crf", "20",
        "-b:v", "0", video,
    ], body.tobytes())
    before, after = metrics(source), metrics(decode(args.ffmpeg, video))
    if (after["frameCount"] != len(body) or after["cornerAlphaMax"] > 0
            or after["seamDelta"] > after["internalP95Delta"]
            or after["seamDelta"] > before["seamDelta"] * .25
            or after["internalMaxDelta"] > before["internalMaxDelta"] * 1.25):
        raise RuntimeError(f"Loop quality check failed: {after}")
    pack = json.loads((args.source_pack / "motions.json").read_text(encoding="utf-8"))
    original = next(clip["frames"] for clip in pack["clips"] if clip["slot"] == "idle")
    if len(original) < 3:
        raise ValueError("At least three PNG fallback frames are required")
    frames = original + original[-2:0:-1]
    report = {
        "method": "original forward frames followed by reverse frames without duplicate endpoints",
        "sourceSha256": sha(args.source), "sourceFrames": len(source),
        "fps": fps, "frameCount": len(body), "durationMs": len(body) / fps * 1000,
        "fallbackFrameMs": len(body) / fps * 1000 / len(frames), "frames": frames,
        "before": before, "after": after, "ffmpegSha256": sha(args.ffmpeg),
        "hashes": {"idle.webm": sha(video)}, "characterRedrawn": False,
    }
    (args.output / "idle-loop.json").write_text(
        json.dumps(report, indent=2) + "\n", encoding="utf-8",
    )
    print(json.dumps({"before": before, "after": after}), flush=True)


if __name__ == "__main__":
    main()
