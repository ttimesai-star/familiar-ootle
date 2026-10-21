#!/usr/bin/env python3
"""Records the walk-through page as a silent video: one frame per transaction of the run, plus a title and an end card.
Nothing is staged: every frame is the page showing one real step from docs/observer_view.json.

    pip install playwright && playwright install chromium   # and ffmpeg on PATH
    python scripts/make_video.py docs/index.html docs/walkthrough.mp4 [docs/walkthrough.gif]
"""
import asyncio
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from playwright.async_api import async_playwright

W, H = 1280, 720
STEP_SECONDS = 3.6
CARD_SECONDS = 4.5

CARD = """<!doctype html><meta charset="utf-8"><style>
body{{margin:0;width:{w}px;height:{h}px;display:flex;align-items:center;justify-content:center;background:#0e1015;
color:#e8eaee;font:22px/1.5 system-ui,Segoe UI,Roboto,Arial,sans-serif}}
.c{{max-width:1000px}} h1{{font-size:56px;margin:0 0 12px}} p{{color:#9aa2b1;margin:8px 0}} b{{color:#a48cff}}
code{{font:20px ui-monospace,Consolas,monospace;color:#e8eaee}}</style><div class="c">{body}</div>"""


async def capture(page_html: Path, frames: Path, n_steps: int, title: str, end: str) -> list:
    shots = []
    async with async_playwright() as p:
        b = await p.chromium.launch()
        pg = await b.new_page(viewport={"width": W, "height": H}, device_scale_factor=1)
        await pg.set_content(CARD.format(w=W, h=H, body=title))
        f = frames / "000_title.png"
        await pg.screenshot(path=str(f))
        shots.append((f, CARD_SECONDS))
        url = page_html.resolve().as_uri()
        for i in range(1, n_steps + 1):
            await pg.goto(f"{url}?capture=1&step={i}")
            await pg.wait_for_timeout(250)
            f = frames / f"{i:03d}.png"
            await pg.screenshot(path=str(f))
            shots.append((f, STEP_SECONDS))
        await pg.set_content(CARD.format(w=W, h=H, body=end))
        f = frames / "999_end.png"
        await pg.screenshot(path=str(f))
        shots.append((f, CARD_SECONDS))
        await b.close()
    return shots


def ffmpeg(*args):
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", *args], check=True)


def main():
    page_html, out_mp4 = Path(sys.argv[1]), Path(sys.argv[2])
    out_gif = Path(sys.argv[3]) if len(sys.argv) > 3 else None
    data = page_html.read_text(encoding="utf-8")
    steps = json.loads(data.split('id="run-data">', 1)[1].split("</script>", 1)[0])
    n_ok = sum(1 for s in steps if s["ok"])
    title = (
        "<h1>Familiar</h1><p>A private, network-enforced allowance for AI agents on Tari Ootle.</p>"
        f"<p><b>{len(steps)} real transactions</b> on the Esmeralda testnet: {n_ok} accepted, "
        f"{len(steps) - n_ok} refused by the network.</p><p>Part 1: attacks on the leash. Part 2: an AI agent that "
        "obeys injected instructions.</p><p>Left: what happened. Right: what anyone reading the chain can see.</p>"
    )
    end = (
        "<h1>What stayed hidden</h1><p>How much the budget held and had left. Who was behind each payee root. "
        "That a recall path existed, until it was used.</p><p>What the network refused: every injected payment, "
        "with an exact reason.</p><p>Prototype, unaudited, testnet only. MIT.</p>"
        "<p><code>github.com/ttimesai-star/familiar-ootle</code></p>"
    )
    with tempfile.TemporaryDirectory() as tmp:
        frames = Path(tmp)
        shots = asyncio.run(capture(page_html, frames, len(steps), title, end))
        lst = frames / "list.txt"
        lines = []
        for f, d in shots:
            lines += [f"file '{f.as_posix()}'", f"duration {d}"]
        lines.append(f"file '{shots[-1][0].as_posix()}'")
        lst.write_text("\n".join(lines) + "\n", encoding="utf-8")
        ffmpeg("-f", "concat", "-safe", "0", "-i", str(lst), "-vf", f"fps=25,scale={W}:{H},format=yuv420p",
               "-c:v", "libx264", "-preset", "slow", "-crf", "22", "-movflags", "+faststart", str(out_mp4))
        poster = out_mp4.with_name(out_mp4.stem + "_poster.png")
        shutil.copy(shots[0][0], poster)
        if out_gif:
            pal = frames / "pal.png"
            ffmpeg("-f", "concat", "-safe", "0", "-i", str(lst), "-vf", "fps=2,scale=880:-1:flags=lanczos,palettegen=max_colors=96", str(pal))
            ffmpeg("-f", "concat", "-safe", "0", "-i", str(lst), "-i", str(pal), "-lavfi",
                   "fps=2,scale=880:-1:flags=lanczos[x];[x][1:v]paletteuse=dither=bayer:bayer_scale=4", str(out_gif))
    total = sum(d for _, d in shots)
    print(f"{out_mp4} ({total:.0f} s, {len(shots)} frames)")


if __name__ == "__main__":
    main()
