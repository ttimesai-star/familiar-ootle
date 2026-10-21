#!/usr/bin/env python3
"""A terminal GIF of "Try it", from the log of a real try-it run (no re-enactment).

    gh run view <run id> --log > tryit.log
    python scripts/make_tryit_gif.py tryit.log docs/try_it.gif --run <run id> [--font DejaVuSansMono.ttf]

Takes the lines of the step "a new user tries it" verbatim with their timestamps, shortens long hex ids for width,
colours the network's decisions, and plays the run as a time-lapse with the real elapsed time in the corner.
"""

from __future__ import annotations

import argparse
import re
from datetime import datetime
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

W, H = 1000, 560
PAD = 16
BG = (18, 20, 26)
FG = (215, 220, 228)
DIM = (120, 128, 140)
CMD = (130, 190, 255)
OK = (110, 210, 130)
NO = (240, 110, 110)
STEP = "a new user tries it"
SPEEDUP = 8  # real seconds per GIF second


def parse(log: str) -> list[tuple[float, str]]:
    out, t0 = [], None
    for raw in log.splitlines():
        parts = raw.split("\t", 2)
        if len(parts) < 3 or STEP not in parts[1]:
            continue
        m = re.match(r"(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d+)?)Z ?(.*)", parts[2].lstrip("\ufeff"))
        if not m:
            continue
        ts = datetime.fromisoformat(m.group(1)[:26])
        text = m.group(2)
        if text.startswith("##[") or text.startswith("[36;1m") or "\x1b[36;1m" in text or text.startswith("shell:"):
            continue  # the runner's echo of the script and its env
        t0 = t0 or ts
        out.append(((ts - t0).total_seconds(), text))
    # keep from the first command on
    first = next(i for i, (_, t) in enumerate(out) if t.startswith("$ familiar"))
    base = out[first][0]
    return [(t - base, x) for t, x in out[first:]]


def shorten(text: str) -> str:
    text = re.sub(r"https://explorer\.tari\.mw/tx/([0-9a-f]{10})[0-9a-f]+", r"explorer/tx/\1…", text)
    text = re.sub(r"\b(component_[0-9a-f]{8})[0-9a-f]{40,}", r"\1…", text)
    text = re.sub(r"\b([0-9a-f]{12})[0-9a-f]{40,}\b", r"\1…", text)
    return text


def colour(text: str):
    if text.startswith("$ familiar"):
        return CMD
    if "REFUSED" in text or "refused" in text.split(":")[0]:
        return NO
    if "ACCEPTED" in text:
        return OK
    return FG


def wrap(text: str, width: int) -> list[str]:
    return [text[i : i + width] for i in range(0, max(len(text), 1), width)]


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("log")
    ap.add_argument("out")
    ap.add_argument("--run", required=True)
    ap.add_argument("--font", default=None)
    a = ap.parse_args()
    lines = parse(Path(a.log).read_text(encoding="utf-8", errors="replace"))
    font_path = a.font or next(
        (p for p in ["C:/Windows/Fonts/DejaVuSansMono.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"] if Path(p).exists()),
        None,
    )
    font = ImageFont.truetype(font_path, 14) if font_path else ImageFont.load_default()
    small = ImageFont.truetype(font_path, 12) if font_path else font
    cw = font.getbbox("M")[2]
    lh = 19
    cols = (W - 2 * PAD) // cw
    rows = (H - 2 * PAD - 26) // lh

    frames, durations = [], []
    shown: list[tuple[str, tuple]] = []

    def render(clock: float, cursor: bool) -> Image.Image:
        im = Image.new("RGB", (W, H), BG)
        d = ImageDraw.Draw(im)
        d.text((PAD, PAD - 4), f"familiar try-it on Esmeralda, GitHub Actions run {a.run} (log verbatim, long ids shortened)", fill=DIM, font=small)
        d.text((W - PAD - 150, PAD - 4), f"real time {clock:5.0f} s", fill=DIM, font=small)
        y0 = PAD + 22
        visible = shown[-rows:]
        for i, (t, c) in enumerate(visible):
            d.text((PAD, y0 + i * lh), t, fill=c, font=font)
        if cursor:
            y = y0 + min(len(shown), rows) * lh
            d.rectangle([PAD, y + 2, PAD + cw - 2, y + lh - 3], fill=FG)
        return im

    prev = 0.0
    for t, text in lines:
        text = shorten(text)
        gap = max(t - prev, 0.0)
        if gap > 0.3:  # waiting for the network: show the real elapsed time ticking
            steps = min(int(gap / SPEEDUP / 0.25) + 1, 12)
            for k in range(steps):
                frames.append(render(prev + gap * (k + 1) / steps, k % 2 == 0))
                durations.append(int(gap / SPEEDUP * 1000 / steps) or 40)
        if text.startswith("$ familiar"):
            for n in range(4, len(text) + 4, 8):  # typed
                shown_now = shown + [(text[:n], CMD)]
                saved, shown[:] = list(shown), shown_now
                frames.append(render(t, True))
                durations.append(35)
                shown[:] = saved
        for part in wrap(text, cols):
            shown.append((part, colour(text)))
        frames.append(render(t, False))
        durations.append(260 if text.startswith("$") else 120)
        prev = t
    durations[-1] = 4000
    # a terminal needs few colours: one shared 16-colour palette keeps the file small
    pal = frames[-1].quantize(colors=16)
    frames = [f.quantize(palette=pal, dither=Image.Dither.NONE) for f in frames]
    frames[0].save(a.out, save_all=True, append_images=frames[1:], duration=durations, loop=0, optimize=True)
    print(f"{len(frames)} frames, {sum(durations) / 1000:.0f} s, {lines[-1][0]:.0f} s real -> {a.out}")


if __name__ == "__main__":
    main()
