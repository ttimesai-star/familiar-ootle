#!/usr/bin/env python3
"""A Model Context Protocol (MCP) server that gives an LLM agent one way to spend: through its Familiar leash.

    FAMILIAR_STATE=me.json FAMILIAR_BIN=./familiar python scripts/familiar_mcp.py

Tools:
  pay_through_leash(payee, amount_ut, reason)  the agent asks to pay; the Esmeralda network decides
  leash_status()                               what the wallet holds and the history of decisions

The agent holds no key that can move the budget outside the leash: the tool only calls `familiar pay`, which builds a
`pay_coupons` transaction signed by the agent's one-time key. Whatever text talked the model into a payment, the
leash's rule on-chain is what accepts or refuses it. Stdio transport, JSON-RPC 2.0, no dependencies.
"""

import json
import os
import subprocess
import sys

PROTOCOL = "2024-11-05"
STATE = os.environ.get("FAMILIAR_STATE", "me.json")
BIN = os.environ.get("FAMILIAR_BIN", "familiar")

TOOLS = [
    {
        "name": "pay_through_leash",
        "description": (
            "Pay a shop from your allowance. The allowance is a Familiar leash on the Tari Ootle network: payments "
            "go out as whole coupons, only to allow-listed payees, within a per-payment and per-window cap. The "
            "network may refuse; the result says why."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "payee": {"type": "string", "description": "payee name, e.g. candle-shop"},
                "amount_ut": {"type": "integer", "description": "amount in micro-tTARI (uT)"},
                "reason": {"type": "string", "description": "why you are paying (kept in the log)"},
            },
            "required": ["payee", "amount_ut"],
        },
    },
    {
        "name": "leash_status",
        "description": "Coupons left in the wallet, the payees, and every past decision of the network.",
        "inputSchema": {"type": "object", "properties": {}},
    },
]


def coupons_for(amount_ut, coupon_ut):
    """Same rounding as the client (`cli::coupons_for`): up, at least one."""
    return max(1, -(-int(amount_ut) // max(1, int(coupon_ut))))


def run(args):
    p = subprocess.run([BIN, *args], capture_output=True, text=True, timeout=600)
    return (p.stdout + p.stderr).strip()


def call_tool(name, arguments):
    if name == "pay_through_leash":
        payee = str(arguments.get("payee", "")).strip()
        amount = int(arguments.get("amount_ut", 0))
        if not payee or amount <= 0:
            return "refused before sending: payee and a positive amount_ut are required", True
        state = json.load(open(STATE, encoding="utf-8"))
        n = coupons_for(amount, state.get("coupon_ut", 100000))
        out = run(["pay", STATE, payee, str(n)])
        return f"requested {amount} uT = {n} coupon(s) to {payee}\n{out}", "REFUSED" in out or "Error" in out
    if name == "leash_status":
        return run(["show", STATE]), False
    return f"unknown tool {name}", True


def handle(msg):
    method, mid = msg.get("method"), msg.get("id")
    if method == "initialize":
        result = {
            "protocolVersion": PROTOCOL,
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "familiar-leash", "version": "0.1.0"},
        }
    elif method == "tools/list":
        result = {"tools": TOOLS}
    elif method == "tools/call":
        params = msg.get("params") or {}
        text, is_error = call_tool(params.get("name"), params.get("arguments") or {})
        result = {"content": [{"type": "text", "text": text}], "isError": bool(is_error)}
    elif method == "ping":
        result = {}
    elif mid is None:  # notifications (initialized, cancelled): no reply
        return None
    else:
        return {"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": f"no method {method}"}}
    return {"jsonrpc": "2.0", "id": mid, "result": result}


def main():
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            reply = handle(json.loads(line))
        except Exception as e:  # noqa: BLE001 - a bad request gets an error reply, the server keeps running
            reply = {"jsonrpc": "2.0", "id": None, "error": {"code": -32603, "message": str(e)}}
        if reply is not None:
            sys.stdout.write(json.dumps(reply) + "\n")
            sys.stdout.flush()


if __name__ == "__main__":
    main()
