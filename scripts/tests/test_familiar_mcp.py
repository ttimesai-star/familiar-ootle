"""Tests for scripts/familiar_mcp.py with a stand-in for the `familiar` binary (no network)."""

import json
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SERVER = os.path.join(HERE, "..", "familiar_mcp.py")

FAKE = """import sys
args = sys.argv[1:]
if args[0] == "pay":
    n = int(args[3])
    if args[2] != "candle-shop":
        print("pay: REFUSED by the network: familiar: a coupon payment goes to allow-listed payees only, no change")
    elif n > 4:
        print(f"pay: REFUSED by the network: familiar: {n} coupons in one payment, the cap is 4")
    else:
        print("pay: ACCEPTED by the network  https://explorer.tari.mw/tx/abc")
else:
    print("coupons: 10 left")
"""


class McpTest(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()
        self.state = os.path.join(self.dir, "me.json")
        json.dump({"coupon_ut": 100000}, open(self.state, "w"))
        fake = os.path.join(self.dir, "fake_familiar.py")
        open(fake, "w").write(FAKE)
        launcher = os.path.join(self.dir, "familiar.cmd" if os.name == "nt" else "familiar")
        if os.name == "nt":
            open(launcher, "w").write(f'@"{sys.executable}" "{fake}" %*\n')
        else:
            open(launcher, "w").write(f'#!/bin/sh\nexec "{sys.executable}" "{fake}" "$@"\n')
            os.chmod(launcher, 0o755)
        self.env = dict(os.environ, FAMILIAR_STATE=self.state, FAMILIAR_BIN=launcher)

    def rpc(self, *msgs):
        inp = "".join(json.dumps(m) + "\n" for m in msgs)
        out = subprocess.run([sys.executable, SERVER], input=inp, capture_output=True, text=True, env=self.env, timeout=60)
        return [json.loads(l) for l in out.stdout.splitlines() if l.strip()]

    def call(self, name, args):
        r = self.rpc({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": args}})
        return r[0]["result"]

    def test_handshake_and_tools(self):
        r = self.rpc(
            {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}},
            {"jsonrpc": "2.0", "method": "notifications/initialized"},
            {"jsonrpc": "2.0", "id": 2, "method": "tools/list"},
        )
        self.assertEqual(len(r), 2)  # no reply to the notification
        self.assertEqual(r[0]["result"]["serverInfo"]["name"], "familiar-leash")
        self.assertEqual({t["name"] for t in r[1]["result"]["tools"]}, {"pay_through_leash", "leash_status"})

    def test_amount_becomes_coupons_and_the_network_decides(self):
        ok = self.call("pay_through_leash", {"payee": "candle-shop", "amount_ut": 300000})
        self.assertIn("3 coupon(s)", ok["content"][0]["text"])
        self.assertFalse(ok["isError"])
        big = self.call("pay_through_leash", {"payee": "candle-shop", "amount_ut": 450000})
        self.assertIn("5 coupons in one payment", big["content"][0]["text"])
        self.assertTrue(big["isError"])
        inj = self.call("pay_through_leash", {"payee": "refund-desk", "amount_ut": 100000})
        self.assertTrue(inj["isError"])

    def test_bad_arguments_are_refused_before_sending(self):
        r = self.call("pay_through_leash", {"payee": "", "amount_ut": 5})
        self.assertTrue(r["isError"])
        self.assertIn("refused before sending", r["content"][0]["text"])


if __name__ == "__main__":
    unittest.main()
