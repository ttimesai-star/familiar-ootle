#!/usr/bin/env python3
"""What an observer sees: pulls every transaction of a Familiar run from the public Esmeralda indexer and lists what
is on-chain in the clear and what is hidden. No keys, no wallet, no build: only public data.

    pip install cbor2
    python scripts/observer_view.py docs/esmeralda_run.json docs/observer_view.md [docs/observer_view.json]

The input is the JSON the client writes next to its report (`familiar scenario ... esmeralda_run.md`).
"""

import json
import sys
import time
import urllib.request

INDEXER = "https://ootle-indexer-a.tari.com"
EXPLORER = "https://explorer.tari.mw/tx/"

try:
    import cbor2
except ImportError:  # the pay argument stays undecoded, everything else still works
    cbor2 = None


def fetch(path, tries=4):
    for i in range(tries):
        try:
            req = urllib.request.Request(INDEXER + path, headers={"User-Agent": "familiar-observer/1.0 (curl-compatible)"})
            with urllib.request.urlopen(req, timeout=60) as r:
                return json.load(r)
        except Exception as e:  # noqa: BLE001 - network errors are retried, then reported
            if i == tries - 1:
                return {"error": str(e)}
            time.sleep(3 * (i + 1))


def short(h, n=12):
    h = str(h)
    return h if len(h) <= n else h[:n] + "..."


class Names:
    """Labels for keys and roots, from the run's public ids."""

    def __init__(self, ids):
        self.labels = {}
        ids = ids or {}
        for key, label in [("agent_pk", "agent key"), ("owner_pk", "owner key"), ("stranger_pk", "stranger key")]:
            if ids.get(key):
                self.labels[ids[key]] = label
        if ids.get("budget_root"):
            self.labels[ids["budget_root"]] = "budget root (leash 1)"
        for i, r in enumerate(ids.get("payee_roots") or []):
            self.labels[r] = f"payee {i + 1} root"
        demo = ids.get("agent_demo") or {}
        if demo.get("budget_root"):
            self.labels[demo["budget_root"]] = "budget root (leash 2)"
        if demo.get("coupon_root"):
            self.labels[demo["coupon_root"]] = "coupon root (leash 2)"
        for i, r in enumerate(demo.get("shop_roots") or []):
            self.labels[r] = f"shop root #{i + 1} (single-use)"
        if demo.get("attacker_root"):
            self.labels[demo["attacker_root"]] = "injected 'refund desk' root"

    def of(self, h):
        label = self.labels.get(str(h))
        return f"`{short(h)}` ({label})" if label else f"`{short(h)}`"


# ---------------------------------------------------------------- CBOR (the `pay` argument)


def walk(o):
    yield o
    if isinstance(o, list):
        for i in o:
            yield from walk(i)
    elif cbor2 and isinstance(o, cbor2.CBORTag):
        yield from walk(o.value)


def cbor_leaf(leaf):
    """A spend-condition leaf as CBOR: [[kind, body], ...], kind 0 = access rule, 3 = covenant."""
    parts = []
    # an `all` leaf is one more list level: [[cond, cond]]
    while isinstance(leaf, list) and len(leaf) == 1 and isinstance(leaf[0], list) and leaf[0] and isinstance(leaf[0][0], list):
        leaf = leaf[0]
    for cond in leaf:
        if not (isinstance(cond, list) and cond):
            continue
        kind = cond[0]
        if kind == 0:
            comp = [x.value.hex() for x in walk(cond) if cbor2 and isinstance(x, cbor2.CBORTag) and x.tag == 128]
            if comp:
                parts.append(f"component(component_{short(comp[0], 16)})")
            else:
                keys = [x.hex() for x in walk(cond) if isinstance(x, bytes) and len(x) == 32]
                parts.append(f"public_key({short(keys[0]) if keys else '?'})")
        elif kind == 3:
            nums = [x for x in walk(cond[1:]) if isinstance(x, int) and not isinstance(x, bool) and x > 3]
            parts.append(f"BalancePreserved({nums[0] if nums else '?'})")
        else:
            parts.append(f"condition kind {kind}")
    return " AND ".join(parts) or "?"


def decode_pay(literal):
    """Decodes a `pay(StealthTransferStatement)` argument into inputs, outputs and covenant claims."""
    if cbor2 is None:
        return None
    try:
        st = cbor2.loads(bytes.fromhex(literal))
        inputs_st, outputs_st, _balance, claims = st[0], st[1], st[2], st[3]
        inputs = []
        for inp in inputs_st[0]:
            commitment, witness = inp[0].hex(), inp[1]
            if isinstance(witness, list) and witness and witness[0] == 1:
                leaf, proof = witness[1][0], witness[1][1]
                inputs.append({"commitment": commitment, "leaf": cbor_leaf(leaf), "siblings": len(proof[0])})
            else:
                inputs.append({"commitment": commitment, "leaf": None, "siblings": 0})
        outputs = []
        for out in outputs_st[0]:
            commitment = out[0][0].hex()
            auth_kind, auth_body = out[1][0], out[1][1]
            target = auth_body[0].hex() if isinstance(auth_body, list) else auth_body.hex()
            outputs.append({"commitment": commitment, "auth": "Script" if auth_kind == 1 else "Key", "to": target})
        revealed = [c[1] for c in claims if isinstance(c, list) and len(c) > 1]
        return {"inputs": inputs, "outputs": outputs, "claims": revealed, "revealed_out": outputs_st[1]}
    except Exception as e:  # noqa: BLE001 - an unexpected shape is reported, not fatal
        return {"error": f"cbor decode: {e}"}


# ---------------------------------------------------------------- JSON statements (fee intents, plain transfers)


def json_leaf(leaf):
    parts = []
    for cond in leaf:
        s = json.dumps(cond)
        if "AccessRule" in cond:
            if "Component" in s or "component_" in s:
                comp = [w for w in s.replace('"', " ").split() if w.startswith("component_")]
                parts.append(f"component({short(comp[0], 26) if comp else '?'})")
            else:
                ids = [w for w in s.replace('"', " ").replace("}", " ").split() if len(w) == 64]
                parts.append(f"public_key({short(ids[0]) if ids else '?'})")
        elif "Covenant" in cond:
            cov = cond["Covenant"]
            if isinstance(cov, dict) and "BalancePreserved" in cov:
                parts.append(f"BalancePreserved({cov['BalancePreserved']})")
            else:
                parts.append(f"covenant {s[:80]}")
        else:
            parts.append(s[:60])
    return " AND ".join(parts) or "?"


def json_statement(st):
    inputs = []
    for inp in st["inputs_statement"]["inputs"]:
        w = inp["witness"]
        if isinstance(w, dict) and "ScriptPath" in w:
            sp = w["ScriptPath"]
            inputs.append(
                {"commitment": inp["commitment"], "leaf": json_leaf(sp["leaf"]), "siblings": len(sp["proof"]["siblings"])}
            )
        else:
            inputs.append({"commitment": inp["commitment"], "leaf": None, "siblings": 0})
    outputs = []
    for out in st["outputs_statement"]["outputs"]:
        auth = out["auth"]
        kind = next(iter(auth)) if isinstance(auth, dict) else str(auth)
        outputs.append({"commitment": out["output"]["commitment"], "auth": kind, "to": auth.get(kind, "") if isinstance(auth, dict) else ""})
    claims = [int(c.get("revealed_amount", 0)) for c in st.get("covenant_claims", []) if isinstance(c, dict)]
    rev = st["outputs_statement"].get("revealed_output")
    return {
        "inputs": inputs,
        "outputs": outputs,
        "claims": claims,
        "revealed_in": int(st["inputs_statement"].get("revealed_amount", "0") or 0),
        "revealed_out": rev,
    }


# ---------------------------------------------------------------- per transaction


def describe_statement(d, names, visible, hidden, fee=False):
    for inp in d["inputs"]:
        if inp["leaf"]:
            visible.append(
                f"spends UTXO `{short(inp['commitment'])}` through the leaf **{inp['leaf']}** "
                f"(the spent leaf is revealed)"
            )
            if inp["siblings"]:
                hidden.append(
                    f"what the other {inp['siblings']} leaf(s) of that tree say: only a hash is published (that they exist is visible)"
                )
        else:
            visible.append(f"spends key-path UTXO `{short(inp['commitment'])}`")
    for out in d["outputs"]:
        visible.append(f"new UTXO `{short(out['commitment'])}` locked under {out['auth']} {names.of(out['to'])}")
    hidden.append(("outputs", len(d["outputs"])))
    hidden.append(("inputs", len(d["inputs"])))
    for c in d["claims"]:
        visible.append(f"covenant claim: exactly **{c} uT** leaves the tree in this spend (cleartext, proven)")
    rev = d.get("revealed_out")
    if isinstance(rev, dict):
        what = "fee" if fee else "revealed output"
        visible.append(f"{what}: {rev.get('amount')} uT revealed to key {names.of(rev.get('receiver'))}")
    if d.get("revealed_in"):
        visible.append(f"revealed input: {d['revealed_in']} uT")


def observe(step, names):
    tx_id = step.get("tx_id")
    out = {"tx_id": tx_id, "visible": [], "hidden": [], "error": None}
    if not tx_id:
        out["error"] = "no transaction"
        return out
    data = fetch(f"/transactions/{tx_id}")
    t = data.get("transaction") if isinstance(data, dict) else None
    if not t:
        out["error"] = f"indexer: {data.get('error') if isinstance(data, dict) else data}"
        return out
    body = t["transaction"]["V1"]["body"]["transaction"]
    signer = t["transaction"]["V1"].get("seal_signature", {}).get("public_key")
    summary = t.get("summary") or {}
    out["outcome"] = summary.get("outcome")
    out["fees"] = summary.get("total_fees_paid")
    out["finalized_at"] = summary.get("finalized_at")
    v, h = out["visible"], out["hidden"]
    v.append(f"outcome **{summary.get('outcome')}**, fee {summary.get('total_fees_paid')} uT, at {summary.get('finalized_at')}")
    if signer:
        v.append(f"sealed by {names.of(signer)}")
    for ins in body.get("fee_instructions", []):
        if "StealthTransfer" in ins:
            describe_statement(json_statement(ins["StealthTransfer"]["statement"]), names, v, h, fee=True)
        elif "CallMethod" in ins:
            v.append(f"fee intent calls `{ins['CallMethod']['method']}` on {short(ins['CallMethod']['call'].get('Address'), 26)}")
    rejected = summary.get("outcome") != "Commit" and bool(body.get("instructions"))
    fee_part = len(h)
    if rejected:
        v.append(
            "**the main part was rejected**: only the fee was taken; what follows is public as submitted, but none of "
            "it happened (no UTXO was spent or created)"
        )
    for ins in body.get("instructions", []):
        if "CallMethod" in ins:
            call = ins["CallMethod"]
            v.append(f"calls `{call['method']}` on `{short(call['call'].get('Address'), 26)}`")
            if call["method"] in ("pay", "pay_coupons") and call.get("args"):
                lit = call["args"][0].get("Literal") if isinstance(call["args"][0], dict) else None
                d = decode_pay(lit) if lit else None
                if d and "error" not in d:
                    describe_statement(d, names, v, h)
                    if call["method"] == "pay_coupons" and not d["claims"] and not d.get("revealed_out"):
                        v.append(
                            f"coupon payment: {len(d['inputs'])} coupon(s) spent, **no covenant claim and no revealed "
                            "amount**: nothing in this transaction says how much moved"
                        )
                        h.append(
                            f"the value of the {len(d['inputs'])} coupon(s) named: their denomination is written "
                            "nowhere on-chain"
                        )
                elif d:
                    v.append(d["error"])
            elif call["method"] == "set_coupons":
                v.append(
                    "method arguments are public: the coupon root, the inclusion proof of its errand leaf, coupons per "
                    "window, window length and coupons per payment (not the coupon denomination)"
                )
            elif call.get("args"):
                v.append("method arguments are public")
        elif "CallFunction" in ins:
            cf = ins["CallFunction"]
            v.append(f"calls `{cf.get('function')}` on template `{short(cf.get('address') or cf.get('template_address'), 16)}`")
            if cf.get("args"):
                v.append("function arguments are public (here: the agent and owner keys, the rule, the payee roots)")
        elif "StealthTransfer" in ins:
            describe_statement(json_statement(ins["StealthTransfer"]["statement"]), names, v, h)
        elif "PublishTemplate" in ins or "PublishTemplateBinary" in json.dumps(ins)[:40]:
            v.append("publishes a template (the WASM is public)")
    if rejected:
        # nothing of the main part reached the chain: count only the fee intent's UTXOs (found in review)
        h = h[:fee_part] + [x for x in h[fee_part:] if not isinstance(x, tuple)]
    n_out = sum(x[1] for x in h if isinstance(x, tuple) and x[0] == "outputs")
    n_in = sum(x[1] for x in h if isinstance(x, tuple) and x[0] == "inputs")
    texts = [x for x in h if isinstance(x, str)]
    hidden = []
    if n_in:
        hidden.append(f"the amount in each of the {n_in} spent UTXO(s)")
    if n_out:
        hidden.append(f"the amount in each of the {n_out} new UTXO(s) (Pedersen commitments with range proofs)")
    seen = set()
    hidden += [x for x in texts if not (x in seen or seen.add(x))]
    out["hidden"] = hidden
    return out


def component_state(addr):
    data = fetch(f"/substates/{addr}")
    try:
        c = data["state"]["up"]["substate"]["Component"]
        return c["body"]["state"], data.get("version")
    except Exception:  # noqa: BLE001
        return None, None


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    run = json.load(open(sys.argv[1], encoding="utf-8"))
    names = Names(run.get("ids"))
    steps = run.get("steps", [])
    md = [
        "# What an observer sees",
        "",
        "Generated by `scripts/observer_view.py` from the public Esmeralda indexer (`" + INDEXER + "`), no keys used. "
        "For every transaction of the run: what anyone can read on-chain, and what stays hidden. "
        "Labels such as \"agent key\" come from the run's public ids; on-chain they are bare hex.",
        "",
    ]
    section = None
    for s in steps:
        o = observe(s, names)
        s["observer"] = o
        if s.get("section") != section:
            section = s.get("section")
            md += [f"## {section}", ""]
        link = f"[{short(s['tx_id'])}]({EXPLORER}{s['tx_id']})" if s.get("tx_id") else "-"
        md.append(f"### {s['step']} - {link}")
        if s.get("input_text"):
            md.append(f"Agent read ({s.get('input_source')}): \"{s['input_text']}\"")
            md.append("")
        if s.get("reason"):
            md.append(f"Refused by the network: `{s['reason']}`")
            md.append("")
        if o.get("error"):
            md += [f"(not observed: {o['error']})", ""]
            continue
        md.append("**On-chain, visible to anyone:**")
        md += [f"- {x}" for x in o["visible"]]
        md.append("")
        if o["hidden"]:
            md.append("**Hidden:**")
            md += [f"- {x}" for x in o["hidden"]]
            md.append("")
    ids = run.get("ids") or {}
    demo = ids.get("agent_demo") or {}
    for label, addr in [("leash 1", ids.get("leash")), ("leash 2", demo.get("leash"))]:
        if not addr:
            continue
        state, version = component_state(addr)
        if state is None:
            continue
        md += [
            f"## Public state of {label} (`/substates/{short(addr, 26)}`, version {version})",
            "",
            "Component state on Ootle is public. Fields in order: resource, limit, window_epochs, window_start, "
            "spent, budget_root, max_per_payment, payees, single_use_payees, payments, paused, then coupon mode: "
            "coupon_root, coupon_siblings, coupon_limit, coupon_window_epochs, coupon_window_start, coupons_spent, "
            "max_coupons_per_payment, coupon_payments. No field holds an amount of coupon mode.",
            "",
            "```",
            json.dumps(state)[:1500],
            "```",
            "",
        ]
        run.setdefault("component_state", {})[label] = state
    md += [
        "## Summary",
        "",
        "Visible: that a leash exists and its rule (limit, window, per-spend cap, the allow-listed roots), every "
        "call to it, the exact amount each payment moves out (covenant claim), the root each output is locked under, "
        "the one-time keys that sign and pay fees, and the leaf used for each spend (the errand leaf on payments, the "
        "recall leaf only when the owner recalls).",
        "",
        "Hidden: how much the budget holds and what is left of it (every UTXO amount is a commitment), who is behind "
        "a payee root (a root is a hash of the payee's own conditions), and what the second leaf says (an owner "
        "recall key) until it is used; that the tree has a second leaf is visible as one sibling hash. With single-use payee roots (leash 2), two payments to the same shop go to two "
        "unrelated roots.",
        "",
        "Coupon mode (leash 2): no payment publishes an amount. A payment spends whole coupons through the leaf "
        "`component(<leash>)` without a covenant claim and sends all of their value to allow-listed roots; the leash "
        "counts coupons, not uT. An observer sees how many coupons each payment spent and how many were funded, so "
        "relative sizes are visible (\"this payment was 3 coupons, that one 2\"); the denomination, and so every "
        "absolute amount, is not.",
        "",
        "Leaks in this demo, by design of the demo rather than of Ootle: step 1 funds the one-time agent, owner and "
        "stranger keys from one funder account in a single transaction, so an observer links those keys to that "
        "account and to each other; the owner key also appears in the leash's access rules. A real owner funds the "
        "one-time keys from separate stealth outputs.",
    ]
    open(sys.argv[2], "w", encoding="utf-8", newline="\n").write("\n".join(md) + "\n")
    if len(sys.argv) > 3:
        open(sys.argv[3], "w", encoding="utf-8", newline="\n").write(json.dumps(run, indent=1))
    errors = sum(1 for s in steps if s.get("observer", {}).get("error"))
    print(f"{len(steps)} steps, {errors} not observed -> {sys.argv[2]}")


if __name__ == "__main__":
    main()
