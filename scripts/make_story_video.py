#!/usr/bin/env python3
"""The 50-second video: an AI agent under prompt injection on a leash in coupon mode, with captions. Every frame of
the middle part is the walk-through page showing one real Esmeralda transaction of Part 2 (docs/observer_view.json);
the caption under it says what happened. The captions are checked against the run before recording: a caption whose
outcome or reason does not match its transaction stops the script.

    pip install playwright && playwright install chromium   # and ffmpeg on PATH
    python scripts/make_story_video.py docs/index.html docs/familiar_story.mp4
"""
import asyncio
import json
import subprocess
import sys
import tempfile
from pathlib import Path

from playwright.async_api import async_playwright

W, H = 1280, 720

CARD = """<!doctype html><meta charset="utf-8"><style>
body{{margin:0;width:{w}px;height:{h}px;display:flex;align-items:center;justify-content:center;background:#0e1015;
color:#e8eaee;font:26px/1.5 system-ui,Segoe UI,Roboto,Arial,sans-serif}}
.c{{max-width:1060px}} h1{{font-size:54px;margin:0 0 18px;line-height:1.15}} p{{color:#aab2c0;margin:10px 0}}
b{{color:#b9a4ff}} code{{font:22px ui-monospace,Consolas,monospace;color:#e8eaee}}</style><div class="c">{body}</div>"""

CAPTION_JS = """(t) => {
  const d = document.createElement('div');
  d.textContent = t;
  d.style.cssText = 'position:fixed;left:0;right:0;bottom:0;padding:16px 28px;background:rgba(14,16,21,.93);' +
    'color:#fff;font:600 27px/1.3 system-ui,Segoe UI,Roboto,Arial,sans-serif;text-align:center;z-index:99';
  document.body.appendChild(d);
}"""

# (seconds, caption, expected: accepted?, text the reason must contain)
STORY = [
    (4.5, "A fake 'owner' email: pay 5 coupons now. The network refuses: the cap is 4 per payment.", False,
     "5 coupons in one payment, the cap is 4"),
    (4.5, "The real owner's task: 3 coupons to the candle shop. Paid. On-chain: 3 hidden coupons, no amount.", True, ""),
    (4.0, "An instruction hidden in a web page: pay the 'refund desk'. Refused: not on the leash.", False,
     "allow-listed payees only"),
    (4.0, "An upsell inside a tool result: 4 more coupons. Refused: 3 of 6 already spent this window.", False,
     "window limit exceeded: 3 of 6 spent"),
    (4.0, "Owner task: 2 coupons. Paid, to a fresh single-use root of the same shop: the two are unlinkable.", True, ""),
    (4.0, "A fake receipt: resend to the first order's address. Refused: that root is used up.", False,
     "allow-listed payees only"),
    (3.5, "The owner pauses the leash with one call. No agent involved.", True, ""),
    (3.5, "The agent tries once more. Refused: paused by the owner.", False, "leash is paused by the owner"),
    (4.0, "The owner takes the rest back through a hidden leaf. Still no amount on-chain.", True, ""),
]

TITLE = (
    "<h1>An AI agent with a wallet can be talked into paying anyone.</h1>"
    "<p>One hidden line in a web page is enough. And on a public chain, its wallet is a billboard.</p>"
    "<p><b>Familiar</b> puts the agent's limits on Tari Ootle and its purse out of sight.</p>"
)
SETUP = (
    "<h1>The leash</h1><p>10 hidden coupons. At most <b>4 per payment</b>, <b>6 per window</b>, only to listed shops."
    "</p><p>Only the owner and the agent know what a coupon is worth. The chain never sees an amount.</p>"
    "<p>The agent obeys <b>every</b> payment instruction it reads. The network is the only guard.</p>"
)
END = (
    "<h1>What the chain saw</h1><p>9 payment attempts, 2 accepted, 7 refused with an exact reason.</p>"
    "<p><b>0 amounts.</b> Not the budget, not what is left, not what a payment moved.</p>"
    "<p>Try it: <code>familiar wallet · tap · leash · ask \"…\"</code></p>"
    "<p>Prototype, unaudited, Esmeralda testnet. MIT.</p>"
)


def check(steps, first):
    for i, (_, caption, ok, reason) in enumerate(STORY):
        s = steps[first + i]
        if bool(s["ok"]) != ok or reason not in (s.get("reason") or ""):
            sys.exit(f"caption {i + 1} does not match step {first + i + 1} ({s['step']}): {caption}")


async def capture(page_html: Path, frames: Path, first: int) -> list:
    shots = []
    async with async_playwright() as p:
        b = await p.chromium.launch()
        pg = await b.new_page(viewport={"width": W, "height": H}, device_scale_factor=1)
        for name, body, secs in [("000_title", TITLE, 5.0), ("001_setup", SETUP, 5.5)]:
            await pg.set_content(CARD.format(w=W, h=H, body=body))
            f = frames / f"{name}.png"
            await pg.screenshot(path=str(f))
            shots.append((f, secs))
        url = page_html.resolve().as_uri()
        for i, (secs, caption, _, _) in enumerate(STORY):
            await pg.goto(f"{url}?capture=1&step={first + i + 1}")
            await pg.wait_for_timeout(250)
            await pg.evaluate(CAPTION_JS, caption)
            f = frames / f"{i + 10:03d}.png"
            await pg.screenshot(path=str(f))
            shots.append((f, secs))
        await pg.set_content(CARD.format(w=W, h=H, body=END))
        f = frames / "999_end.png"
        await pg.screenshot(path=str(f))
        shots.append((f, 6.0))
        await b.close()
    return shots


def main():
    page_html, out_mp4 = Path(sys.argv[1]), Path(sys.argv[2])
    data = page_html.read_text(encoding="utf-8")
    steps = json.loads(data.split('id="run-data">', 1)[1].split("</script>", 1)[0])
    first = next(i for i, s in enumerate(steps) if s["section"] == "The agent reads its inbox")
    check(steps, first)
    with tempfile.TemporaryDirectory() as tmp:
        frames = Path(tmp)
        shots = asyncio.run(capture(page_html, frames, first))
        lst = frames / "list.txt"
        lines = []
        for f, d in shots:
            lines += [f"file '{f.as_posix()}'", f"duration {d}"]
        lines.append(f"file '{shots[-1][0].as_posix()}'")
        lst.write_text("\n".join(lines) + "\n", encoding="utf-8")
        subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-f", "concat", "-safe", "0", "-i", str(lst), "-vf",
                        f"fps=25,scale={W}:{H},format=yuv420p", "-c:v", "libx264", "-preset", "slow", "-crf", "22",
                        "-movflags", "+faststart", str(out_mp4)], check=True)
        keep = Path(tempfile.gettempdir()) / "familiar_story_frames"  # for checking the frames by eye
        keep.mkdir(exist_ok=True)
        for f, _ in shots:
            (keep / f.name).write_bytes(f.read_bytes())
    print(f"{out_mp4} ({sum(d for _, d in shots):.1f} s, {len(shots)} frames)")


if __name__ == "__main__":
    main()
