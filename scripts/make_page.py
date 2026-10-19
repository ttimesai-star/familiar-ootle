#!/usr/bin/env python3
"""Builds docs/index.html (the project page served by GitHub Pages) from docs/esmeralda_run.md.

The transaction tables on the page are parsed from the run report, so the page never shows an id that is not in the
report. Usage: python scripts/make_page.py [--repo https://github.com/<owner>/<name>]
"""
from __future__ import annotations

import argparse
import html
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RUN = ROOT / "docs" / "esmeralda_run.md"
OUT = ROOT / "docs" / "index.html"
EXPLORER = "https://explorer.tari.mw/tx/"
INDEXER = "https://ootle-indexer-a.tari.com/transactions/"

ROW = re.compile(r"^\| (?P<step>[^|]+) \| \[(?P<short>[0-9a-f.]+)\]\((?P<url>[^)]+)\) \| (?P<outcome>[^|]+) \| (?P<note>[^|]*) \|$")


def esc(s: str) -> str:
    return html.escape(s, quote=True)


LONG_HEX = re.compile(r"(?<![0-9a-f])((?:component_)?[0-9a-f]{12})[0-9a-f]{20,}")


def code_spans(s: str, shorten: bool = False) -> str:
    """Escapes text and turns `x` into <code>x</code>; optionally shortens long hex ids (full ids are listed below
    the table)."""
    if shorten:
        s = LONG_HEX.sub(lambda m: m[1] + "…", s)
    parts = s.split("`")
    return "".join(f"<code>{esc(p)}</code>" if i % 2 else esc(p) for i, p in enumerate(parts))


def parse_run(text: str) -> dict:
    rows = []
    for line in text.splitlines():
        m = ROW.match(line.strip())
        if not m:
            continue
        txid = m["url"].rsplit("/", 1)[-1]
        outcome = m["outcome"].strip()
        reason = ""
        if " - refused: " in outcome:
            outcome, reason = outcome.split(" - refused: ", 1)
            reason = reason.strip().strip("`")
            reason = re.sub(r"^At instruction #\d+: ", "", reason)
            reason = reason.replace("Template error: ", "")
        rows.append({"step": m["step"].strip(), "txid": txid, "outcome": outcome.strip(), "reason": reason,
                     "note": m["note"].strip()})
    epoch = re.search(r"Run at epoch (\d+)", text)
    policy = re.search(r"Policy: (.+?)\.\n", text)
    ids = {k: re.search(rf"{k}: `([^`]+)`", text) for k in ("Template", "Leash", "Budget root")}
    return {
        "rows": rows,
        "epoch": epoch[1] if epoch else "?",
        "policy": policy[1] if policy else "",
        "ids": {k: (v[1] if v else "") for k, v in ids.items()},
    }


def tx_cell(txid: str) -> str:
    return (f'<span class="tx"><a href="{EXPLORER}{txid}" title="{txid}"><code>{txid[:12]}…</code></a>'
            f' <a class="raw" href="{INDEXER}{txid}" title="indexer record (JSON)">json</a></span>')


def table(rows: list[dict], attack: bool) -> str:
    head = ("<tr><th>Attack</th><th>Transaction</th><th>Refused by</th><th>Reason returned by the network</th></tr>"
            if attack else "<tr><th>Step</th><th>Transaction</th><th>Outcome</th><th>Note</th></tr>")
    body = []
    for r in rows:
        step = re.sub(r"^\d+[a-z]?\. ", "", r["step"])
        if attack:
            by = re.sub(r"^refused( by)?:? ?", "", r["note"]).split(":")[0] or "the network"
            body.append(f"<tr><td data-l=\"Attack\"><span>{code_spans(step)}</span></td>"
                        f"<td data-l=\"Transaction\">{tx_cell(r['txid'])}</td>"
                        f"<td data-l=\"Refused by\"><span>{code_spans(by)}</span></td>"
                        f"<td data-l=\"Reason\" class=\"reason\"><span><code>{esc(r['reason'])}</code></span></td></tr>")
        else:
            body.append(f"<tr><td data-l=\"Step\"><span>{code_spans(step)}</span></td>"
                        f"<td data-l=\"Transaction\">{tx_cell(r['txid'])}</td>"
                        f"<td data-l=\"Outcome\"><span><span class=\"ok\">{esc(r['outcome'])}</span></span></td>"
                        f"<td data-l=\"Note\"><span>{code_spans(r['note'], shorten=True)}</span></td></tr>")
    return f'<div class="tw"><table class="tx-table"><thead>{head}</thead><tbody>{"".join(body)}</tbody></table></div>'


def page(run: dict, repo: str, video: bool, run_date: str) -> str:
    rows = run["rows"]
    attacks = [r for r in rows if r["reason"]]
    setup = [r for r in rows if not r["reason"]]
    ids = run["ids"]
    video_block = (
        '<video controls preload="metadata" poster="familiar_poster.png" src="familiar.mp4">'
        '<a href="familiar.mp4">familiar.mp4</a></video>'
        '<p class="small">90-second walkthrough. Narration is synthetic (ElevenLabs); terminal and tables are from '
        'the run above.</p>'
        if video else "")
    return f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Familiar: a private, network-enforced allowance for AI agents on Tari Ootle</title>
<meta name="description" content="An AI agent's budget held in hidden stealth UTXOs; the Ootle engine enforces a per-payment cap, a per-window limit, a payee allow-list and owner recall. Testnet prototype.">
<meta name="color-scheme" content="light dark">
<style>
:root {{
  --bg: #fbfbfa; --fg: #16181d; --muted: #59606d; --line: #dcdfe4; --card: #ffffff; --code: #f1f2f4;
  --accent: #5b3fd1; --ok: #116b3a; --okbg: #e3f4ea; --no: #9b1c1c; --nobg: #fbe9e9;
  --mono: ui-monospace, "SF Mono", "Cascadia Mono", Menlo, Consolas, monospace;
  --sans: system-ui, -apple-system, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
}}
@media (prefers-color-scheme: dark) {{
  :root {{ --bg: #0e1015; --fg: #e8eaee; --muted: #9aa2b1; --line: #2a2f3a; --card: #151821; --code: #1d212c;
    --accent: #a48cff; --ok: #6fdc9a; --okbg: #14301f; --no: #ff9a9a; --nobg: #3a1717; }}
}}
* {{ box-sizing: border-box; }}
html {{ -webkit-text-size-adjust: 100%; }}
body {{ margin: 0; background: var(--bg); color: var(--fg); font: 16px/1.55 var(--sans); }}
main {{ max-width: 1040px; margin: 0 auto; padding: 32px 20px 64px; }}
h1 {{ font-size: clamp(28px, 5vw, 40px); line-height: 1.15; margin: 0 0 8px; letter-spacing: -0.01em; }}
h2 {{ font-size: 22px; margin: 44px 0 12px; padding-top: 8px; border-top: 1px solid var(--line); }}
h3 {{ font-size: 17px; margin: 24px 0 8px; }}
p, li {{ max-width: 76ch; }}
a {{ color: var(--accent); text-underline-offset: 2px; }}
a:focus-visible {{ outline: 2px solid var(--accent); outline-offset: 2px; border-radius: 2px; }}
code {{ font: 0.88em/1.4 var(--mono); background: var(--code); padding: 1px 5px; border-radius: 4px; overflow-wrap: anywhere; }}
pre {{ background: var(--code); padding: 14px 16px; border-radius: 8px; overflow-x: auto; font: 13.5px/1.5 var(--mono); }}
pre code {{ background: none; padding: 0; }}
.lead {{ font-size: 19px; color: var(--muted); margin: 0 0 16px; }}
.status {{ display: inline-block; font: 13px/1.4 var(--mono); border: 1px solid var(--line); border-radius: 999px; padding: 4px 12px; color: var(--muted); }}
nav.links {{ display: flex; flex-wrap: wrap; gap: 8px 18px; margin: 18px 0 0; }}
nav.links a {{ display: inline-flex; align-items: center; min-height: 44px; font-weight: 600; }}
.tw {{ overflow-x: auto; border: 1px solid var(--line); border-radius: 8px; background: var(--card); }}
table {{ border-collapse: collapse; width: 100%; font-size: 14.5px; }}
th, td {{ text-align: left; vertical-align: top; padding: 9px 12px; border-bottom: 1px solid var(--line); }}
tr:last-child td {{ border-bottom: 0; }}
th {{ font-size: 13px; color: var(--muted); font-weight: 600; white-space: nowrap; }}
td.reason code {{ background: var(--nobg); color: var(--no); }}
.ok {{ font: 12.5px/1.4 var(--mono); background: var(--okbg); color: var(--ok); padding: 2px 6px; border-radius: 4px; }}
a.raw {{ font-size: 12px; color: var(--muted); }}
.tx {{ white-space: nowrap; }}
.small {{ font-size: 14px; color: var(--muted); }}
.grid2 {{ display: grid; grid-template-columns: 1fr 1fr; gap: 16px; }}
.card {{ border: 1px solid var(--line); border-radius: 8px; background: var(--card); padding: 14px 16px; }}
.card h3 {{ margin-top: 0; }}
.card ul {{ margin: 0; padding-left: 18px; }}
.flow {{ display: grid; grid-template-columns: 1fr 44px 1.25fr 44px 1fr; align-items: center; gap: 0; }}
.box {{ border: 1px solid var(--line); border-radius: 8px; background: var(--card); padding: 12px 14px; font-size: 14.5px; }}
.box b {{ display: block; margin-bottom: 4px; }}
.box.leash {{ border-color: var(--accent); }}
.arrow {{ text-align: center; color: var(--muted); font: 20px var(--mono); }}
.utxo {{ margin-top: 16px; }}
.leaf {{ font: 13px/1.6 var(--mono); white-space: pre; overflow-x: auto; background: var(--code); padding: 10px 12px; border-radius: 6px; }}
video {{ width: 100%; max-width: 960px; border-radius: 8px; border: 1px solid var(--line); background: #000; }}
footer {{ margin-top: 48px; font-size: 14px; color: var(--muted); border-top: 1px solid var(--line); padding-top: 16px; }}
@media (max-width: 760px) {{
  .grid2 {{ grid-template-columns: 1fr; }}
  .flow {{ grid-template-columns: 1fr; }}
  .arrow {{ transform: rotate(90deg); height: 32px; line-height: 32px; }}
  th, td {{ padding: 8px 9px; }}
  table.tx-table thead {{ display: none; }}
  table.tx-table, table.tx-table tbody, table.tx-table tr, table.tx-table td {{ display: block; width: 100%; }}
  table.tx-table tr {{ border-bottom: 1px solid var(--line); padding: 6px 0; }}
  table.tx-table tr:last-child {{ border-bottom: 0; }}
  table.tx-table td {{ border: 0; padding: 3px 12px; display: grid; grid-template-columns: 92px 1fr; gap: 8px; }}
  table.tx-table td::before {{ content: attr(data-l); color: var(--muted); font-size: 12.5px; padding-top: 2px; }}
}}
@media (prefers-reduced-motion: reduce) {{ * {{ scroll-behavior: auto; transition: none; }} }}
</style>
</head>
<body>
<main>
<header>
<span class="status">prototype · unaudited · Esmeralda testnet only</span>
<h1>Familiar</h1>
<p class="lead">A private, network-enforced allowance for AI agents on Tari Ootle. The agent can spend, but only up to
the limits the owner set, only to payees the owner listed, and the owner can take the budget back at any moment.
The Ootle engine enforces this, not the agent's code and not a server.</p>
<nav class="links" aria-label="Project links">
<a href="{esc(repo)}">Source code (MIT)</a>
<a href="#esmeralda">Live testnet run</a>
<a href="{esc(repo)}/blob/main/THREAT_MODEL.md">Threat model</a>
<a href="#run">Run it yourself</a>
</nav>
</header>

<section aria-labelledby="why">
<h2 id="why">The problem</h2>
<p>The Ootle wallet daemon can already hand an AI agent an API key with scopes such as
<code>TransactionSend_&lt;account&gt;</code>. A scope says <em>what</em> the agent may do, not <em>how much</em>: a key
that may send may send the whole account. If the agent is prompt-injected, or its host is compromised, the limit is
whatever the account holds. On a transparent chain the agent's wallet is also public: its balance, every service it
pays and who funds it.</p>
<p>Familiar moves the limits into the chain and keeps the money out of sight.</p>
</section>

<section aria-labelledby="enforces">
<h2 id="enforces">What the network enforces</h2>
<div class="tw"><table>
<tr><th>Rule</th><th>Enforced by</th></tr>
<tr><td>at most <code>max_per_payment</code> leaves the budget in one payment</td><td>native covenant <code>BalancePreserved(max_per_payment)</code> inside each budget UTXO's spend condition</td></tr>
<tr><td>at most <code>limit</code> per window of <code>window_epochs</code> epochs</td><td>the leash component, charging the amount the covenant itself proves (<code>covenant_claims</code>), never an argument</td></tr>
<tr><td>payments only to allow-listed payees</td><td>the leash component: every output must be locked under the budget root (change) or an allow-listed payee root</td></tr>
<tr><td>the budget cannot be spent anywhere except through the leash</td><td>the spend condition requires <code>component(&lt;this leash&gt;)</code>; a plain transfer or another component is refused by the engine</td></tr>
<tr><td>the owner can recall everything without the agent</td><td>a second leaf <code>public_key(&lt;owner one-time key&gt;)</code>, hidden on-chain until used</td></tr>
</table></div>
</section>

<section aria-labelledby="how">
<h2 id="how">How it works</h2>
<div class="flow" role="img" aria-label="Agent calls pay on the leash component, which checks the spend and sends hidden outputs to allow-listed payees, with change back to the budget.">
  <div class="box"><b>Agent</b>one-time key, assumed compromised. Builds a stealth spend and calls <code>pay(spend)</code>.</div>
  <div class="arrow" aria-hidden="true">→</div>
  <div class="box leash"><b>Leash component</b>
    caller is the agent key · not paused · nothing revealed · every output under the budget root or an allow-listed
    payee root · window limit on the covenant-proven amount · then executes the spend from its own frame</div>
  <div class="arrow" aria-hidden="true">→</div>
  <div class="box"><b>Outputs</b>payee: stealth output under an opaque root<br>change: back under the budget root</div>
</div>
<div class="grid2 utxo">
  <div class="card"><h3>Every budget UTXO</h3>
    <p class="small">Amount hidden by a Pedersen commitment. Locked under a pure condition tree
    <code>Script(root)</code>, no key path:</p>
    <p class="leaf">errand = component(&lt;leash&gt;)
     AND BalancePreserved(max)
recall = public_key(&lt;owner key&gt;)</p></div>
  <div class="card"><h3>Owner</h3>
    <ul><li>funds the budget from stealth UTXOs;</li>
    <li>sets the rule, the payees and the budget root (owner key only);</li>
    <li>pauses or resumes the leash;</li>
    <li>recalls the budget through the recall leaf with a plain stealth transfer. The agent is left with nothing to
    spend.</li></ul></div>
</div>
</section>

<section aria-labelledby="esmeralda">
<h2 id="esmeralda">Live on the Esmeralda testnet</h2>
<p>Every step below is a real transaction, run on {esc(run_date)} at epoch {esc(run["epoch"])} by the
<code>familiar scenario</code> client in GitHub Actions. Policy: {esc(run["policy"])}. Keys were generated for the run
and discarded. tTARI has no value. Each id links to the explorer; <em>json</em> is the indexer record.</p>
{video_block}
<h3>Try to break it</h3>
<p class="small">Each attack was submitted as a transaction. <code>FeeIntentCommit</code> means the network charged the
fee and rejected the instructions. The reason text is what the network returned to the client.</p>
{table(attacks, True)}
<h3>Setup, payments and recall</h3>
{table(setup, False)}
<p class="small">Template <code>{esc(ids["Template"])}</code><br>Leash <code>{esc(ids["Leash"])}</code><br>
Budget root <code>{esc(ids["Budget root"])}</code></p>
</section>

<section aria-labelledby="privacy">
<h2 id="privacy">What an observer sees</h2>
<div class="grid2">
<div class="card"><h3>Visible</h3><ul>
<li>that a payment happened, its epoch and the leash component it went through;</li>
<li>the amount of each payment (the covenant proves the exact outflow in clear);</li>
<li>the window rule and how much of the current window is used;</li>
<li>the payee's condition root (the same root for repeat payments to one payee).</li></ul></div>
<div class="card"><h3>Hidden</h3><ul>
<li>the budget and what is left of it;</li>
<li>who the payee is behind its root;</li>
<li>who the owner is (one-time key, stealth funding);</li>
<li>that an owner recall path exists, until it is used.</li></ul></div>
</div>
<p><b>Honest limits.</b> Payment amounts are public. Repeat payments to one payee can be linked. The agent's fee coin
can link its payments unless it is itself stealth-funded. The window rule is public. The leash bounds what a
compromised agent can spend; it does not make the agent anonymous. Prototype, not audited.</p>
</section>

<section aria-labelledby="run">
<h2 id="run">Run it yourself</h2>
<p>Engine tests run the template on the same engine validators run (<code>tari_template_test_tooling</code> 0.46,
<code>tari_template_lib</code> 0.34). Every refusal test asserts the exact reason. The test engine needs about 2 GB
of RAM.</p>
<pre><code>git clone {esc(repo)}.git
cd {esc(repo.rstrip("/").rsplit("/", 1)[-1])}

# engine tests (template + window arithmetic)
cargo test -p familiar-tests -p familiar_core

# the template as published (wasm32, then wasm-opt)
rustup target add wasm32-unknown-unknown
(cd templates/familiar &amp;&amp; cargo build --release --target wasm32-unknown-unknown)
wasm-opt -Oz --enable-bulk-memory --strip-debug --strip-producers --strip-target-features \\
  templates/familiar/target/wasm32-unknown-unknown/release/familiar.wasm -o familiar.min.wasm

# the whole testnet story: publish, leash, fund, pay, five attacks, recall
FAMILIAR_FUNDER=&lt;account-secret-hex&gt;:&lt;view-secret-hex&gt; \\
  cargo run -p familiar-cli --release -- scenario familiar.min.wasm esmeralda_run.md</code></pre>
<p class="small">The funder is any Esmeralda account holding tTARI (<code>familiar keygen</code> creates one; fund it
through the testnet faucet). The workflows in <code>.github/workflows/</code> run the same commands.</p>
</section>

<footer>
<p>MIT licence. Written for the Tari Ootle October 2026 build contest ("Spooky Secrets"), starting 18 October 2026.
Developed with AI coding agents; design decisions, review and submission by the author. Not affiliated with Tari
Labs.</p>
</footer>
</main>
</body>
</html>
"""


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo", default="https://github.com/ttimesai-star/familiar")
    ap.add_argument("--run-date", default="18 October 2026")
    a = ap.parse_args()
    run = parse_run(RUN.read_text(encoding="utf-8"))
    if len(run["rows"]) < 10:
        raise SystemExit(f"parsed only {len(run['rows'])} rows from {RUN}")
    video = (OUT.parent / "familiar.mp4").exists()
    OUT.write_text(page(run, a.repo.rstrip("/"), video, a.run_date), encoding="utf-8", newline="\n")
    (OUT.parent / ".nojekyll").write_text("", encoding="utf-8")
    print(f"{OUT}: {len(run['rows'])} transactions, video={'yes' if video else 'no'}")


if __name__ == "__main__":
    main()
