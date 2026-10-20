# Familiar threat model

Prototype, unaudited, testnet only. This file lists what Familiar defends against, how, and which test shows it.
Every "Refused" row is an engine test in `tests/leash.rs` that asserts the exact rejection reason.

## Actors

| Actor | Holds | Trusted for |
|---|---|---|
| Owner | one-time owner key (recall leaf + owner-only methods) | everything: it funds the budget and can take it back |
| Agent | one-time agent key (the `pay` rule) | nothing beyond the leash: it is assumed compromised |
| Payee | its own condition tree (root on the allow-list) | receiving payments |
| Observer | the public chain | nothing |

## Compromised agent (the main case)

The attacker has the agent key and the budget UTXO secrets (masks and values), can build any statement, deploy any
template and submit any transaction.

| Attack | Defence | Result | Test |
|---|---|---|---|
| Spend more than `limit` in a window | the component charges the covenant-proven outflow against the window | Refused: `familiar: window limit exceeded: 80 of 100 spent, 20 left, payment of 30 refused` | `window_limit_refuses_with_numbers_then_reopens_next_window` |
| Move a large amount in one spend | native covenant `BalancePreserved(max_per_payment)` in the UTXO itself | Refused: `covenant not satisfied: BalancePreserved(40)` | `single_payment_over_the_covenant_allowance_is_refused` |
| Pay an address that is not on the allow-list | every output must be under the budget root or an allow-listed payee root | Refused: `familiar: output to a payee not on the leash` | `payment_to_a_payee_not_on_the_leash_is_refused`, `payout_to_a_plain_key_is_refused` |
| Keep the change but add a key path to escape the tree | outputs must be exactly `Script(root)` | Refused (same message) | `change_with_an_added_key_path_is_refused` |
| Take value out as a revealed bucket | revealed inputs and outputs are not allowed in `pay` | Refused: `familiar: revealed outputs are not allowed` | `revealed_output_is_refused` |
| Spend the budget with a plain stealth transfer, bypassing the leash | errand leaf requires `component(<leash>)` | Refused by the engine (`No active call frame`) | `spending_the_budget_with_a_plain_transfer_is_refused` |
| Deploy a hostile component that spends the budget from its own frame | `component(<leash>)` names one component address | Refused: `Access Denied: native.stealth_utxo.spend` | `spending_the_budget_from_a_hostile_component_is_refused` |
| Replay an accepted payment | the spent UTXOs no longer exist | Refused (substate not found) | `replaying_a_payment_is_refused` |
| Spend UTXOs from an old and a new budget root in one spend (after a rotation) to move 2 x the per-spend cap | the leash sums the covenant claims and refuses above `max_per_payment` (found in review) | Refused: `familiar: payment of 80 over the per-payment cap 40` | `spend_across_two_budget_roots_is_capped_by_the_leash` |
| Lie about the amount | the amount is read from `covenant_claims`, which the engine proves; an understated claim fails natively | covered by the engine's own covenant tests | - |
| Change the rule, the payees, the budget root, or pause/resume | owner-only method rules | Refused: `Access Denied` | `admin_methods_are_owner_only` |
| Use the recall leaf | `public_key(<owner>)` | Refused: `Access Denied: native.stealth_utxo.spend` | `stranger_cannot_use_the_recall_leaf` |

What a compromised agent *can* do: spend up to `limit` per window to allow-listed payees until the owner pauses the
leash or recalls the budget. That is the bound the leash promises.

## Other actors

| Attack | Defence | Test |
|---|---|---|
| A stranger calls `pay` with a valid statement | `pay` requires the agent key as a signer | `a_stranger_cannot_pay` |
| The owner key calls `pay` (bypassing the agent) | the component has no owner (`OwnerRule::None`); an owner would bypass method rules, found and fixed during development | `the_owner_cannot_pay_as_the_agent` |
| The owner registers the budget root as a payee (self-dealing loop) | refused in `set_payees` and `set_budget_root` | `budget_root_cannot_be_a_payee` |
| The owner shrinks the window mid-window, resetting what was spent (found in review) | `set_policy` restarts the window at the current epoch and carries `spent` over | `shrinking_the_window_keeps_what_was_spent` |
| A leash without a budget root | `pay` refuses | `pay_without_a_budget_root_is_refused` |

## Privacy: what leaks

| Leaks | Why | Mitigation |
|---|---|---|
| Amount of each payment | the covenant allowance is enforced on a cleartext outflow | none in this version |
| Payee root, linkable across payments to the same payee | allow-list entries are fixed roots | a payee can rotate roots (`set_payees`) |
| The leash component and its per-window rule | public component state | the budget itself is not in the component |
| The agent's fee source | the fee coin is a key-path UTXO of the agent key | fund fees from fresh stealth outputs |
| Timing | public chain | none |

Not leaked: the budget, what is left of it, who funded it, who the payees are behind their roots, and that a recall
path exists until it is used.

## Out of scope

Side channels on the client machine; loss of the owner key (the budget is then stuck under the errand leaf, spendable
only within the leash rules); front-running of a pay statement (it can only pay the same allow-listed payee); the
engine and the covenant implementation themselves.
