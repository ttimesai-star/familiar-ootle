#!/usr/bin/env python3
"""Builds docs/index.html, the project page (GitHub Pages, branch `page`, folder `docs/`).

Everything about the run comes from docs/observer_view.json (written by scripts/observer_view.py from the public
indexer), so the page never shows a transaction id or a claim that is not in the run.

    python scripts/make_page.py [--repo https://github.com/<owner>/<name>]
"""
from __future__ import annotations

import argparse
import html
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DATA = ROOT / "docs" / "observer_view.json"
OUT = ROOT / "docs" / "index.html"


def esc(s) -> str:
    return html.escape(str(s), quote=True)


def code_spans(s: str) -> str:
    parts = str(s).split("`")
    return "".join(f"<code>{esc(p)}</code>" if i % 2 else esc(p) for i, p in enumerate(parts))


def clean_reason(r: str) -> str:
    r = re.sub(r"^At instruction #\d+: ", "", r or "")
    return r.replace("Template error: ", "")


def actor_of(step: dict) -> str:
    for line in step.get("observer", {}).get("visible", []):
        m = re.search(r"sealed by `[^`]+`(?: \(([^)]+)\))?", line)
        if m:
            return {"agent key": "agent", "owner key": "owner", "stranger key": "stranger"}.get(m[1] or "", "funder")
    return "?"


def page_steps(run: dict) -> list[dict]:
    out = []
    for s in run["steps"]:
        o = s.get("observer") or {}
        out.append({
            "section": s.get("section", ""),
            "step": re.sub(r"^[A-Z]?\d+[a-z]?\. ", "", s["step"]),
            "tx": s.get("tx_id", ""),
            "ok": bool(s.get("accepted")),
            "outcome": o.get("outcome") or s.get("outcome", ""),
            "reason": clean_reason(s.get("reason", "")),
            "note": s.get("note", ""),
            "source": s.get("input_source"),
            "text": s.get("input_text"),
            "actor": actor_of(s),
            "visible": o.get("visible", []),
            "hidden": o.get("hidden", []),
            "at": o.get("finalized_at", ""),
        })
    return out


CSS = r"""
:root { --bg:#fbfbfa; --fg:#16181d; --muted:#59606d; --line:#dcdfe4; --card:#fff; --code:#f1f2f4; --accent:#5b3fd1;
  --ok:#116b3a; --okbg:#e3f4ea; --no:#9b1c1c; --nobg:#fbe9e9; --warn:#7a4b00; --warnbg:#fff3d6;
  --mono:ui-monospace,"SF Mono","Cascadia Mono",Menlo,Consolas,monospace;
  --sans:system-ui,-apple-system,"Segoe UI",Roboto,"Helvetica Neue",Arial,sans-serif; }
@media (prefers-color-scheme: dark) { :root { --bg:#0e1015; --fg:#e8eaee; --muted:#9aa2b1; --line:#2a2f3a; --card:#151821;
  --code:#1d212c; --accent:#a48cff; --ok:#6fdc9a; --okbg:#14301f; --no:#ff9a9a; --nobg:#3a1717; --warn:#ffd27a; --warnbg:#3a2a0c; } }
* { box-sizing:border-box; } body { margin:0; background:var(--bg); color:var(--fg); font:16px/1.55 var(--sans); }
main { max-width:1080px; margin:0 auto; padding:32px 20px 64px; }
h1 { font-size:clamp(28px,5vw,40px); line-height:1.15; margin:0 0 8px; }
h2 { font-size:22px; margin:44px 0 12px; padding-top:8px; border-top:1px solid var(--line); }
h3 { font-size:17px; margin:20px 0 8px; } p, li { max-width:78ch; }
a { color:var(--accent); text-underline-offset:2px; }
code { font:.88em/1.4 var(--mono); background:var(--code); padding:1px 5px; border-radius:4px; overflow-wrap:anywhere; }
pre { background:var(--code); padding:14px 16px; border-radius:8px; overflow-x:auto; font:13.5px/1.5 var(--mono); }
pre code { background:none; padding:0; }
.lead { font-size:19px; color:var(--muted); margin:0 0 16px; }
.pill { display:inline-block; font:13px/1.4 var(--mono); border:1px solid var(--line); border-radius:999px; padding:4px 12px; color:var(--muted); }
nav.links { display:flex; flex-wrap:wrap; gap:8px 18px; margin:16px 0 0; }
nav.links a { display:inline-flex; align-items:center; min-height:44px; font-weight:600; }
.card { border:1px solid var(--line); border-radius:10px; background:var(--card); padding:14px 16px; }
.grid2 { display:grid; grid-template-columns:1fr 1fr; gap:16px; }
.small { font-size:14px; color:var(--muted); }
video { width:100%; border-radius:10px; border:1px solid var(--line); background:#000; display:block; }
/* walk-through */
.walk { display:grid; grid-template-columns:1fr 1fr; gap:16px; }
.stephead { display:flex; flex-wrap:wrap; align-items:center; gap:8px 12px; margin-bottom:6px; }
.sec { font:12.5px/1.4 var(--mono); color:var(--muted); text-transform:uppercase; letter-spacing:.04em; }
.num { font:13px/1.4 var(--mono); color:var(--muted); }
.title { font-size:21px; font-weight:700; margin:2px 0 10px; line-height:1.25; }
.badge { font:12.5px/1.4 var(--mono); padding:3px 8px; border-radius:6px; white-space:nowrap; }
.badge.ok { background:var(--okbg); color:var(--ok); } .badge.no { background:var(--nobg); color:var(--no); }
.actor { font:12.5px/1.4 var(--mono); padding:3px 8px; border-radius:6px; border:1px solid var(--line); }
.read { border-left:4px solid var(--warn); background:var(--warnbg); padding:10px 12px; border-radius:6px; margin:10px 0; }
.read .src { font:12.5px var(--mono); color:var(--warn); display:block; margin-bottom:4px; }
.reason { background:var(--nobg); color:var(--no); padding:10px 12px; border-radius:6px; font:14px/1.45 var(--mono); overflow-wrap:anywhere; }
.reason b { font-family:var(--sans); display:block; margin-bottom:4px; }
.obs h3 { margin-top:0; } .obs ul { margin:4px 0 12px; padding-left:18px; font-size:14.5px; }
.obs li { margin:3px 0; overflow-wrap:anywhere; } .obs .hid li::marker { content:"◼ "; color:var(--accent); }
.controls { display:flex; flex-wrap:wrap; gap:8px; align-items:center; margin:12px 0; }
button { font:600 15px var(--sans); min-height:44px; padding:0 16px; border-radius:8px; border:1px solid var(--line);
  background:var(--card); color:var(--fg); cursor:pointer; }
button.primary { background:var(--accent); color:#fff; border-color:var(--accent); }
button:focus-visible, a:focus-visible { outline:2px solid var(--accent); outline-offset:2px; }
.dots { display:flex; flex-wrap:wrap; gap:6px; margin:6px 0 0; }
.dot { width:30px; height:30px; min-height:30px; padding:0; border-radius:6px; font:12px var(--mono); }
.dot.ok { border-color:var(--ok); } .dot.no { border-color:var(--no); } .dot.cur { background:var(--accent); color:#fff; }
.links2 { font-size:14px; margin-top:10px; } .links2 a { margin-right:14px; }
.state { font:13.5px/1.6 var(--mono); } .state td { padding:4px 10px 4px 0; vertical-align:top; overflow-wrap:anywhere; }
table.plain { border-collapse:collapse; width:100%; font-size:14.5px; }
table.plain th, table.plain td { text-align:left; vertical-align:top; padding:8px 10px; border-bottom:1px solid var(--line); }
footer { margin-top:48px; font-size:14px; color:var(--muted); border-top:1px solid var(--line); padding-top:16px; }
@media (max-width:820px) { .walk, .grid2 { grid-template-columns:1fr; } }
/* capture mode for the video: the walk-through alone, larger */
body.capture main { max-width:1240px; padding:28px 32px; } body.capture header, body.capture section:not(#walk-sec), body.capture footer { display:none; }
body.capture #walk-sec h2, body.capture .controls, body.capture .dots, body.capture .links2, body.capture .walkintro { display:none; }
body.capture .title { font-size:28px; } body.capture .obs ul { font-size:14.5px; } body.capture .obs li { margin:1px 0; } body.capture .reason { font-size:17px; }
body.capture .read { font-size:18px; } body.capture #walk-sec { border:0; }
body.capture .capbar { display:flex; } .capbar { display:none; justify-content:space-between; font:15px var(--mono); color:var(--muted); margin-bottom:14px; }
"""

JS = r"""
const STEPS = JSON.parse(document.getElementById('run-data').textContent);
const EXPLORER = 'https://explorer.tari.mw/tx/', INDEXER = 'https://ootle-indexer-a.tari.com';
const q = new URLSearchParams(location.search);
let cur = 0, timer = null;
const $ = (id) => document.getElementById(id);
const esc = (s) => String(s ?? '').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const md = (s) => esc(s).replace(/\*\*(.+?)\*\*/g, '<b>$1</b>').replace(/`([^`]+)`/g, '<code>$1</code>');
function render() {
  const s = STEPS[cur];
  $('w-sec').textContent = s.section;
  $('w-num').textContent = `step ${cur + 1} of ${STEPS.length}`;
  $('w-title').textContent = s.step;
  $('w-actor').textContent = `signed by: ${s.actor}`;
  const b = $('w-badge'); b.className = 'badge ' + (s.ok ? 'ok' : 'no');
  b.textContent = s.ok ? `${s.outcome}: accepted` : `${s.outcome}: refused`;
  $('w-read').hidden = !s.text;
  if (s.text) { $('w-src').textContent = `the agent read (${s.source}):`; $('w-text').textContent = `"${s.text}"`; }
  $('w-reason').hidden = !s.reason;
  $('w-reason-t').textContent = s.reason;
  $('w-note').innerHTML = md(s.note);
  $('w-vis').innerHTML = s.visible.map(x => `<li>${md(x)}</li>`).join('');
  $('w-hid').innerHTML = s.hidden.length ? s.hidden.map(x => `<li>${md(x)}</li>`).join('') : '<li>nothing beyond the fee coin change</li>';
  $('w-links').innerHTML = s.tx ? `<a href="${EXPLORER}${s.tx}" target="_blank" rel="noopener">open in the explorer</a>` +
    `<a href="${INDEXER}/transactions/${s.tx}" target="_blank" rel="noopener">raw indexer record (JSON)</a>` +
    `<code title="${s.tx}">${s.tx.slice(0, 16)}…</code>` : '';
  $('cap-l').textContent = `Familiar on Esmeralda · ${s.section}`; $('cap-r').textContent = `${cur + 1}/${STEPS.length}`;
  document.querySelectorAll('.dot').forEach((d, i) => { d.classList.toggle('cur', i === cur); d.setAttribute('aria-current', i === cur ? 'step' : 'false'); });
  if (!q.has('capture')) history.replaceState(null, '', `#step-${cur + 1}`);
}
function go(i) { cur = Math.max(0, Math.min(STEPS.length - 1, i)); render(); }
function play() {
  if (timer) { clearInterval(timer); timer = null; $('w-play').textContent = 'Play'; return; }
  $('w-play').textContent = 'Pause';
  timer = setInterval(() => { if (cur >= STEPS.length - 1) { play(); return; } go(cur + 1); }, 4500);
}
function init() {
  const dots = $('w-dots');
  STEPS.forEach((s, i) => {
    const d = document.createElement('button'); d.className = 'dot ' + (s.ok ? 'ok' : 'no'); d.textContent = i + 1;
    d.title = `${s.step} (${s.ok ? 'accepted' : 'refused'})`; d.setAttribute('aria-label', `step ${i + 1}: ${s.step}`);
    d.onclick = () => go(i); dots.appendChild(d);
  });
  $('w-prev').onclick = () => go(cur - 1); $('w-next').onclick = () => go(cur + 1); $('w-play').onclick = play;
  document.addEventListener('keydown', e => { if (e.target.tagName === 'INPUT') return;
    if (e.key === 'ArrowRight') go(cur + 1); if (e.key === 'ArrowLeft') go(cur - 1); });
  const m = location.hash.match(/^#step-(\d+)$/); const qs = q.get('step');
  if (q.has('capture')) document.body.classList.add('capture');
  // Without a step in the URL, open at the first prompt injection: that is the story; step 1 is a faucet.
  const firstInbox = Math.max(0, STEPS.findIndex(s => s.section === 'The agent reads its inbox'));
  go(qs ? +qs - 1 : (m ? +m[1] - 1 : firstInbox));
  $('live-btn').onclick = live;
}
const FIELDS = ['resource', 'limit (uT per window)', 'window_epochs', 'window_start (epoch)', 'spent in this window (uT)',
  'budget_root', 'max_per_payment (uT)', 'payee roots', 'single_use_payees', 'payments', 'paused',
  'coupon_root', 'coupon tree proof', 'coupons per window', 'coupon window (epochs)', 'coupon window start',
  'coupons spent in this window', 'coupons per payment', 'coupon payments'];
function val(v) {
  if (v && v['@cbor'] === 'bytes') return v.hex.slice(0, 16) + '…';
  if (v && v['@cbor'] === 'tag') return val(v.value);
  if (Array.isArray(v)) return v.length ? v.map(val).join(', ') : '(none left)';
  return String(v);
}
async function live() {
  const out = $('live-out'); out.textContent = 'reading the indexer…';
  const leashes = JSON.parse(document.getElementById('leash-data').textContent);
  let html = '';
  for (const [label, addr] of leashes) {
    try {
      const r = await fetch(`${INDEXER}/substates/${addr}`); const j = await r.json();
      const st = j.state.up.substate.Component.body.state;
      html += `<h3>${esc(label)} <span class="small">version ${esc(j.version)}, read just now</span></h3><table class="state">` +
        st.map((v, i) => `<tr><td>${esc(FIELDS[i] || i)}</td><td>${esc(val(v))}</td></tr>`).join('') + '</table>';
    } catch (e) { html += `<p>${esc(label)}: could not read (${esc(e)})</p>`; }
  }
  html += '<p class="small">No balance anywhere: the budget is in hidden UTXOs, not in the component.</p>';
  out.innerHTML = html;
}
init();
"""


def build(run: dict, repo: str, video: str | None) -> str:
    steps = page_steps(run)
    ids = run.get("ids") or {}
    demo = ids.get("agent_demo") or {}
    leashes = [["leash 1 (attacks)", ids.get("leash")], ["leash 2 (the AI agent)", demo.get("leash")]]
    leashes = [x for x in leashes if x[1]]
    n_ok = sum(1 for s in steps if s["ok"])
    n_no = len(steps) - n_ok
    first_at = next((s["at"] for s in steps if s["at"]), "")
    data = json.dumps(steps).replace("</", "<\\/")
    leash_json = json.dumps(leashes).replace("</", "<\\/")
    video_html = (
        f'<video controls preload="metadata" playsinline poster="walkthrough_poster.png"><source src="{esc(video)}" '
        'type="video/mp4"></video><p class="small">Silent, made from this page: each frame is one real transaction '
        'of the run below. No narration, no edits to the data.</p>'
        if video else ""
    )
    return f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Familiar: a private, network-enforced allowance for AI agents on Tari Ootle</title>
<meta name="description" content="An AI agent's budget in hidden stealth UTXOs on Tari Ootle. The network enforces a per-payment cap, a window limit, a payee allow-list and owner recall; a prompt-injected agent is refused on-chain. Testnet prototype.">
<meta name="color-scheme" content="light dark">
<style>{CSS}</style>
</head>
<body>
<main>
<header>
<span class="pill">prototype · unaudited · Esmeralda testnet only</span>
<h1>Familiar</h1>
<p class="lead">A private allowance for AI agents on Tari Ootle. The agent can spend only up to the owner's limits,
only to payees the owner listed, and the owner can take the budget back at any moment. The network enforces it, not
the agent's code. In coupon mode no amount ever reaches the chain: not the budget, not what is left, not what a
payment moved. An observer sees that a payment happened and how many hidden coupons it spent, never what a coupon is
worth, nor who is behind a payee.</p>
<nav class="links" aria-label="Project links">
<a href="#walk-sec">Walk through the live run</a>
<a href="#video-sec">Video</a>
<a href="#live-sec">Read the leash live</a>
<a href="{esc(repo)}">Source code (MIT)</a>
<a href="{esc(repo)}/blob/main/THREAT_MODEL.md">Threat model</a>
<a href="#quick">Try it yourself</a>
</nav>
</header>

<section id="walk-sec" aria-labelledby="walk">
<h2 id="walk">Walk through the run, transaction by transaction</h2>
<p class="walkintro">{len(steps)} real transactions on the Esmeralda testnet, from {esc(first_at)} UTC: {n_ok} accepted,
{n_no} refused by the network. Part 1 attacks a leash in amount mode directly. Part 2, where this page opens, hands
a leash in coupon mode to an AI agent that obeys every payment instruction it reads, including ones injected into a web
page, an email and a fake receipt: no payment of Part 2 publishes an amount. For each step: what happened, and what
anyone reading the chain can see. Use the arrows, the numbers, or the keyboard (← →).</p>
<div class="controls">
<button id="w-prev" aria-label="previous step">← Prev</button>
<button id="w-next" class="primary" aria-label="next step">Next →</button>
<button id="w-play">Play</button>
</div>
<div class="dots" id="w-dots" aria-label="steps"></div>
<div class="capbar"><span id="cap-l"></span><span id="cap-r"></span></div>
<div class="walk" style="margin-top:14px">
<div class="card" aria-live="polite">
<div class="stephead"><span class="sec" id="w-sec"></span><span class="num" id="w-num"></span></div>
<div class="title" id="w-title"></div>
<div class="stephead"><span class="badge" id="w-badge"></span><span class="actor" id="w-actor"></span></div>
<div class="read" id="w-read" hidden><span class="src" id="w-src"></span><span id="w-text"></span></div>
<div class="reason" id="w-reason" hidden><b>Refused by the network:</b><span id="w-reason-t"></span></div>
<p class="small" id="w-note"></p>
<div class="links2" id="w-links"></div>
</div>
<div class="card obs">
<h3>What an observer sees on-chain</h3><ul id="w-vis"></ul>
<h3>What stays hidden</h3><ul class="hid" id="w-hid"></ul>
<p class="small">Decoded from the public indexer by <code>scripts/observer_view.py</code>; no keys used.</p>
</div>
</div>
</section>

<section id="video-sec" aria-labelledby="video">
<h2 id="video">Video</h2>
{video_html}
</section>

<section id="live-sec" aria-labelledby="livet">
<h2 id="livet">Read the leash live</h2>
<p>Component state on Ootle is public, so this page can read it straight from the Esmeralda indexer, now, from your
browser. This is everything the leash itself discloses: the rule, the window, the roots. Not a balance.</p>
<button id="live-btn" class="primary">Read both leashes from the indexer</button>
<div id="live-out" class="card" style="margin-top:12px">Not read yet.</div>
</section>

<section aria-labelledby="why">
<h2 id="why">The problem</h2>
<p>The Ootle wallet daemon can hand an AI agent an API key with scopes such as <code>TransactionSend_&lt;account&gt;</code>.
A scope says <em>what</em> the agent may do, not <em>how much</em>: a key that may send may send the whole account. An
agent that reads the web can be told, in text hidden in a page, to do exactly that. On a transparent chain its wallet is
also public: balance, every service it pays, who funds it.</p>
<p>Familiar moves the limits into the chain and keeps the purse out of sight.</p>
</section>

<section aria-labelledby="enf">
<h2 id="enf">What the network enforces</h2>
<table class="plain">
<tr><th>Rule</th><th>Enforced by</th></tr>
<tr><td>at most <code>max_per_payment</code> per payment</td><td>native covenant <code>BalancePreserved(max_per_payment)</code> in each budget UTXO's spend condition</td></tr>
<tr><td>at most <code>limit</code> per window of epochs</td><td>the leash component, charging the amount the covenant proves, never an argument</td></tr>
<tr><td>payments only to allow-listed payees</td><td>every output under the budget root (change) or an allow-listed payee root</td></tr>
<tr><td>optionally, each payee root is paid once</td><td>single-use mode strikes a paid root off the list: repeat payments to one shop land on unrelated roots</td></tr>
<tr><td>the budget moves only through the leash</td><td>the spend condition requires <code>component(&lt;this leash&gt;)</code></td></tr>
<tr><td>coupon mode: at most N coupons per payment and per window, no amount on-chain</td><td><code>pay_coupons</code>
checks from each input's revealed leaf and inclusion proof that it is a coupon of this leash, refuses change and claims,
and counts coupons; Pedersen balance proves that no value was created</td></tr>
<tr><td>the owner recalls everything, without the agent</td><td>a hidden second leaf <code>public_key(&lt;owner one-time key&gt;)</code>, spent without a covenant claim so the amount is not published</td></tr>
</table>
<p class="small">Every budget UTXO: amount hidden by a Pedersen commitment, locked under a pure condition tree
<code>Script(root)</code> with two leaves: <code>errand = component(leash) AND BalancePreserved(cap)</code> and
<code>recall = public_key(owner)</code>.</p>
</section>

<section aria-labelledby="priv">
<h2 id="priv">Privacy: visible, hidden, and what leaks</h2>
<div class="grid2">
<div class="card"><h3>Visible to anyone</h3><ul>
<li>each payment, its time, the leash and the one-time key that signed it;</li>
<li>amount mode: the exact amount of each payment (the covenant claim); coupon mode: only how many coupons it spent;</li>
<li>the errand leaf of each spent UTXO, and the whole component state (rule, window usage, roots).</li></ul></div>
<div class="card"><h3>Hidden</h3><ul>
<li>how much the budget holds and has left (commitments; the recall carries no claim);</li>
<li>coupon mode: what any payment moved: the coupon denomination is written nowhere on-chain;</li>
<li>who is behind a payee root;</li>
<li>what the second leaf says (the owner's recall) until it is used; only its hash is published;</li>
<li>with single-use roots: that two payments went to the same shop.</li></ul></div>
</div>
<p><b>Leaks, named.</b> In amount mode payment amounts are public; in coupon mode their relative sizes are
(3 coupons, then 2). The window usage is public (component state). Payments of one
leash are linked by the leash and the agent key. In this demo the one-time keys are funded from one account in one
transaction, which links them; a real owner funds them from separate stealth outputs. Prototype, not audited.</p>
</section>

<section aria-labelledby="quick">
<h2 id="quick">Try it yourself</h2>
<p>Your own leash in coupon mode on Esmeralda, from a fresh key, in about five minutes (prebuilt client in the
release, or <code>cargo build -p familiar-cli --release</code>):</p>
<pre><code>familiar wallet me.json        # a fresh testnet key, written only to me.json
familiar tap    me.json        # 5 tTARI from the Familiar tap
familiar leash  me.json        # a leash in coupon mode with 10 hidden coupons
familiar pay    me.json candle-shop 2
familiar ask    me.json "any text, e.g. a hidden instruction to pay refund-desk"
familiar pause  me.json; familiar recall me.json; familiar show me.json</code></pre>
<p>For an LLM agent: <code>scripts/familiar_mcp.py</code> is an MCP server whose only spending tool,
<code>pay_through_leash</code>, goes through the same leash.</p>
<pre><code># 1. check what an observer sees, from the public indexer only (no keys, no Rust)
git clone {esc(repo)}.git && cd {esc(repo.rstrip('/').rsplit('/', 1)[-1])}
pip install cbor2
python scripts/observer_view.py docs/esmeralda_run.json observer.md

# 2. engine tests (Rust stable + wasm32 target, about 2 GB of RAM)
cargo test --workspace

# 3. the whole run again on Esmeralda: in a fork, add secret FAMILIAR_FUNDER (a funded testnet account),
#    then Actions -> claim-and-run -> Run workflow (skip_claim = true). Report, observer view and the
#    template wasm come back as the job summary and artifact.</code></pre>
</section>

<footer>
<p>MIT licence. Written for the Tari Ootle October 2026 build contest ("Spooky Secrets"), starting 18 October 2026.
Developed with AI coding agents; design, review and submission by the author. Not affiliated with Tari Labs.</p>
</footer>
</main>
<script type="application/json" id="run-data">{data}</script>
<script type="application/json" id="leash-data">{leash_json}</script>
<script>{JS}</script>
</body>
</html>
"""


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo", default="https://github.com/ttimesai-star/familiar-ootle")
    ap.add_argument("--data", default=str(DATA))
    ap.add_argument("--out", default=str(OUT))
    ap.add_argument("--video", default=None, help="video file name next to index.html")
    a = ap.parse_args()
    run = json.loads(Path(a.data).read_text(encoding="utf-8"))
    Path(a.out).write_text(build(run, a.repo, a.video), encoding="utf-8", newline="\n")
    print(f"wrote {a.out}")


if __name__ == "__main__":
    main()
