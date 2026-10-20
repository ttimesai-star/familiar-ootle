# Familiar

**A private, network-enforced allowance for AI agents on Tari Ootle.**

> Status: **prototype, unaudited, testnet only.** Built for the Tari Ootle October 2026 contest, "Spooky Secrets".
> No real money. Do not use on mainnet.

A witch's familiar runs errands. It can spend, but only what the leash allows, only at the shops it is sent to,
and the witch can call it home at any moment. Nobody watching the street can tell how much the familiar carries,
who sent it, or that it can be called home at all.

Familiar gives an AI agent a wallet with exactly those properties, enforced by the Ootle engine, not by the agent's
code and not by a server:

| The network enforces | How |
|---|---|
| at most `limit` µT per window of `window_epochs` epochs | the `Familiar` component tracks the window and refuses the spend |
| at most `max_per_payment` µT in a single spend | native covenant `BalancePreserved(max_per_payment)` in the UTXO's spend condition (per budget root), plus the leash's own cap on the sum of the covenant claims of a spend (covers a spend across an old and a new budget root) |
| payments only to allow-listed payees | every output of a payment must be locked under the budget root (change) or an allow-listed payee root |
| the budget cannot be spent anywhere except inside the leash | the errand leaf requires `component(<this leash>)`; a plain transfer or a hostile component is refused by the engine |
| the owner can take everything back at any time, without the agent | a second leaf `public_key(<owner one-time key>)` |

## Why

The Ootle wallet daemon already lets you give an agent an API key with scopes such as `TransactionSend_<account>`
(guide: *API Keys for AI Agents*). A scope says *what* the agent may do, not *how much*: a key that may send may send
everything. On a transparent chain, the agent's wallet is also a billboard: its balance, every service it pays and
its owner are public, which makes a well-funded agent a target.

Familiar keeps the leash on-chain and the secrets off it.

## What an observer sees, and what stays hidden

| Visible on-chain | Hidden |
|---|---|
| that a payment happened, its epoch, and the leash component it went through | the **budget** and **what is left** (stealth UTXOs, Pedersen commitments) |
| the amount of each payment (the covenant proves the exact cleartext outflow) | **who the payee is** (a stealth output; only an opaque condition root is visible) |
| the per-window rule (`limit`, `window_epochs`) and how much of the current window is used | **who the owner is** (one-time key, funding from stealth UTXOs) |
| the condition root of the payee (the same root for repeat payments to the same payee) | that an **owner recall path exists**, until it is used (MAST: unexercised leaves stay hidden) |

Honest limits: payment amounts are public (the per-payment allowance is enforced on a cleartext outflow); repeat
payments to one payee share a root and can be linked; the agent's transaction fee source can link its payments if it
is not itself stealth-funded; the per-window rule is public. The leash bounds the damage of a compromised agent, it
does not make the agent anonymous.

## Live on the Esmeralda testnet

The whole story ran on Esmeralda on 18 October 2026, every step a real transaction:
[`docs/esmeralda_run.md`](docs/esmeralda_run.md) (links to the explorer). In short: the template was published, a
leash created, its budget funded as two hidden UTXOs, the agent paid two allow-listed payees, then five attacks were
submitted and each was refused on-chain by the mechanism that should refuse it:

| Attack | Refused by | Engine message |
|---|---|---|
| one spend over the per-spend cap | the covenant in the UTXO | `covenant not satisfied: BalancePreserved(400000)` |
| over the window limit | the leash component | `familiar: window limit exceeded: 800000 of 1000000 spent, 200000 left, payment of 300000 refused` |
| pay itself instead of a payee | the leash component | `familiar: output to a payee not on the leash` |
| spend the budget outside the leash | the engine | `No active call frame` |
| a stranger calls `pay` | the engine | `Access Denied: call component method 'pay' ...` |

Then the agent paid what was left of the window and the owner recalled the rest through the recall leaf, which had
not been visible on-chain until that moment. Reproduce: `client/` (`familiar scenario`), see `.github/workflows/`.

## How it works

Every budget UTXO commits a pure condition tree (`Script(root)`, no key path) with two leaves:

```
errand = component(<leash>) AND BalancePreserved(max_per_payment)
recall = public_key(<owner one-time key>)
```

* `pay(spend)` (signed by the agent's one-time key) checks the shape of the spend: no revealed inputs or outputs,
  every output under the budget root or an allow-listed payee root, at least one payee output. The amount charged
  to the window is the outflow the covenant itself proves (`covenant_claims`), never an argument. Then it executes
  the spend from inside its own frame, which is the only place the errand leaf can be satisfied.
* `pause` / `resume` / `set_policy` / `set_payees` / `set_budget_root(root, max_per_payment)`: owner key only.
  `set_policy` restarts the window at the current epoch and carries over what was already spent, so changing the
  rule mid-window cannot reset the budget.
* `status`: public view of the rule and the current window (not the budget).
* Recall: the owner spends the UTXOs through the recall leaf with a plain stealth transfer; the agent is left with
  nothing to spend.

## Tests (real Ootle engine, in-process)

`cargo test --workspace` runs the template on the same engine validators run (`tari_template_test_tooling` 0.46,
`tari_template_lib` 0.34). Every refusal test asserts the exact reason. See `tests/leash.rs`:

* happy path: three payments to two payees, the payee then spends its payout;
* window: refused with numbers (`80 of 100 spent, 20 left, payment of 30 refused`), reopens next window;
* covenant: single payment over `max_per_payment` refused by the engine (`BalancePreserved(40)`);
* outside the leash: hostile component refused (`Access Denied: native.stealth_utxo.spend`), plain transfer refused;
* shape: unlisted payee, plain-key payout, change with an added key path, revealed output: refused;
* auth: only the agent pays; admin methods owner-only; pause/resume; recall by owner, not by a stranger;
* construction: bad policy, empty or duplicate payees, agent key equal to owner key;
* found in review (see below): a spend across two budget roots is capped by the leash; shrinking the window keeps what
  was spent; two UTXOs under one root are one covenant partition; zero payment refused; duplicate payees refused.

### Found in review

An external review (Jules, Gemini) of the 19.10 snapshot found two medium issues. Both were first reproduced as
passing proof-of-concept tests on the engine (CI run 39209529251), then fixed with red-then-green tests:

1. After a budget-root rotation, one spend could take UTXOs from the old and the new root; the covenant is checked
   per root, so the spend could move `2 x max_per_payment`. Fix: the leash stores `max_per_payment` and refuses a
   spend whose summed covenant claims exceed it (`spend_across_two_budget_roots_is_capped_by_the_leash`).
2. `set_policy` with a shorter window in the middle of a window reset `spent` to zero. Fix: `restart_window`
   starts a new window at the current epoch and keeps what was spent (`shrinking_the_window_keeps_what_was_spent`).

Checked and not an issue: the client never prints secret values (`funder_secret` reads the env var, keygen writes
the secret only to a file).

`core/` holds the window arithmetic as a pure crate with a randomized property test (accepted payments never exceed
the limit in any aligned window).

## Layout

```
templates/familiar   the template (Rust -> WASM)
core                 window/limit arithmetic, shared by the template and tests
templates/test_faucet, templates/mallory   test-only helpers (never published)
tests/leash.rs       engine tests
docs/                spec and evidence
```

## License

MIT. All code in this repository was written for this contest, starting 18 October 2026. Developed with AI coding
agents; the idea, design decisions, review and submission are the author's.
