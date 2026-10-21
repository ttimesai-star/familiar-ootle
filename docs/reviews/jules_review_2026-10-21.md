# Jules review 21.10 (session 713469587911428299), extracted from its patch

# Security Review Report: Tari Ootle Familiar Snapshot

**Target:** `projects/contests/tari_ootle/review_snapshot_familiar`  
**Date:** 2026-10-21  
**Auditor:** Jules  
**Scope:** Read-only security audit of Familiar template, Rust client, observer script, engine tests, and documentation.

---

## Executive Summary

Familiar implements a private, network-enforced allowance mechanism for AI agents on Tari Ootle. The budget is held in stealth UTXOs locked under a 2-leaf script condition tree:
1. **Errand Leaf:** `component(<leash>) AND BalancePreserved(max_per_payment)`
2. **Recall Leaf:** `public_key(<owner key>)`

Overall, the architecture provides strong, engine-enforced bounds on agent spending while preserving UTXO balance confidentiality. The covenant and access rules effectively prevent unauthorized withdrawals, host-component hijacking, and limit overruns across budget root rotations.

This review identified 1 MEDIUM-severity finding regarding single-use payee root removal within a single transaction, and 3 LOW-severity findings covering observer tool output counting, parser edge cases, and documentation clarity regarding on-chain privacy guarantees.

---

## Detailed Evaluation by Section

### 1. Template Core Logic (`templates/familiar/src/lib.rs`)

* **`set_single_use_payees` & `paid_roots` Removal in `pay`:**
  * **Paid Root Survival & Single-Tx Reuse:** In `pay()`, outputs are validated against `self.payees` during an iteration over `spend.stealth_outputs()`. If a single `pay` transaction contains multiple stealth outputs locked under the same payee root $R$, `self.payees.contains(root)` evaluates to `true` for all of them. The root $R$ is added to `paid_roots` once, and `self.payees.retain(|root| !paid_roots.contains(root))` is executed only at the end of `pay()`. Consequently, an agent can pay the same "single-use" root multiple times within a single transaction. (See **Finding 1**).
  * **Toggling & Owner Top-Up:** If `single_use_payees` is `false` when a payment occurs, `payees` is not modified. Toggling `single_use_payees` to `true` later does not retroactively purge previously paid roots. Furthermore, `set_payees` allows the owner to re-add previously paid roots.
  * **Interaction with `MAX_PAYEES`:** `set_payees()` enforces `!payees.is_empty()`, `payees.len() <= MAX_PAYEES`, and no duplicates. When single-use mode is enabled, `pay()` reduces `self.payees`. When `self.payees` becomes empty, subsequent payments fail cleanly with `"familiar: output to a payee not on the leash"`. The owner can top up `self.payees` at any time up to `MAX_PAYEES`.

* **Assertion `spend without a covenant claim`:**
  * `assert!(!spend.covenant_claims.is_empty(), "familiar: spend without a covenant claim");`
  * This assertion guarantees that an agent cannot submit a claimless transaction to `pay()`. Without a claim, `amount` would sum to 0, charging nothing against the window limit. Because the errand leaf contains the `BalancePreserved(max_per_payment)` covenant condition, the Ootle engine would also evaluate and reject a claimless spend at the UTXO level. The explicit component assertion provides a clear, actionable error message before state transition attempts.

* **`status` without `remaining_in_window`:**
  * `status()` returns `spent_in_window`, `limit`, `window_epochs`, `window_start`, and other metadata, but deliberately omits a `remaining` field. This design choice prevents client applications from confusing remaining window allowance with remaining budget UTXO balance (which is hidden off-chain).

---

### 2. Client & Recall Logic (`client/src/main.rs`)

* **`recall_input` Construction:**
  * `recall_input()` constructs `StealthInputWitness` with `condition_root: None` when spending budget UTXOs through the recall leaf (`public_key(<owner key>)`). Because `condition_root` is `None`, the wallet crypto library generates no covenant claims for the transaction statement.

* **Engine Evaluation & Errand Leaf Defense:**
  * The recall leaf contains only an access rule (`public_key`), with no `Covenant` atom. The Ootle engine evaluates covenant claims only when a `Covenant` condition atom is present in the executed script leaf. Therefore, the recall spend succeeds without publishing any covenant claim, keeping the recalled amount completely confidential.
  * Conversely, a spend through the errand leaf (`component(<leash>) AND BalancePreserved(...)`) evaluates a `Covenant` atom. If an attacker attempts a claimless spend through the errand leaf, the engine rejects it natively (`covenant not satisfied`), in addition to `lib.rs`'s explicit assertion check. A claimless spend **cannot** pass through the errand leaf.

---

### 3. Agent Parser & Simulation (`client/src/agent.rs` & `client/src/main.rs`)

* **`agent.rs` Parser:**
  * `parse()` extracts tool calls matching `<verb> <N> uT to <who>`.
  * The parser handles case insensitivity, digit separators (`_`, `,`), and multi-token sentences.
  * **Edge Case:** `unit` is normalized with `name(unit) != "ut"`, stripping punctuation. However, `to` is compared directly via `to != "to"`. If the text contains trailing punctuation on `to` (e.g. `"pay 100 uT to, candle-shop"`), parsing fails (see **Finding 3**).
  * **Multi-word Payees:** `parse()` extracts only the token at `i + 4`. Multi-word names (e.g., `"candle shop"`) are truncated to `"candle"`.
  * **First Match:** `parse()` returns on the first valid payment verb structure found in the input string.

* **`agent_demo` Logic:**
  * The demo accurately models prompt injection across 6 inbox messages (spoofed owner email, legitimate tasks, malicious web comment, shop upsell, fake receipt, post-pause attempt).
  * Single-use payee root rotation is properly simulated using `next_shop.min(2)` and `first_order` tracking, demonstrating network-level rejection of replayed/unlisted payee roots and paused leash status.

---

### 4. Observer View Script (`scripts/observer_view.py`)

* **Labeling Accuracy:**
  * `observer_view.py` correctly decodes CBOR pay statements, JSON transaction statements, component substates, and public keys/roots.
  * Visible and hidden attributes (Pedersen commitments, revealed script leaves, Merkle sibling counts, covenant claims, fee reveals) are accurately categorized for committed transactions.

* **Counting Discrepancy on Rejected Transactions:**
  * For transactions where the main instruction is rejected (e.g., Steps 5a, 6b, 6c, 6d, 6e), `observe()` calls `describe_statement()` on the unexecuted `pay` call arguments. `describe_statement()` appends input/output counts to the hidden list `h`.
  * As a result, `docs/observer_view.md` reports unexecuted statement inputs/outputs as "hidden UTXOs" on-chain for rejected transactions (e.g. reporting 2 spent and 3 new UTXOs for Step 5a, when in reality only 1 fee UTXO was spent and created on-chain) (see **Finding 2**).

---

### 5. Engine Test Suite (`tests/leash.rs`)

All 6 new test functions in `tests/leash.rs` strictly assert what their function names claim:

1. `single_use_payee_root_cannot_be_paid_twice`: Asserts that paying a previously used single-use payee root fails with `"familiar: output to a payee not on the leash"`, while a fresh root succeeds.
2. `single_use_off_by_default_repeat_root_is_accepted`: Asserts that repeat payments to the same root succeed when `single_use_payees` is `false`.
3. `single_use_runs_out_of_roots_until_the_owner_tops_up`: Asserts that an exhausted allow-list rejects payments until `set_payees` adds a new root.
4. `a_refused_payment_does_not_consume_a_single_use_root`: Asserts that a transaction rejected by covenant rules does not consume the payee root from the allow-list.
5. `owner_recall_without_a_claim_does_not_reveal_what_is_left`: Asserts that recall statements contain no covenant claims and no revealed outputs (`covenant_claims.is_empty()`), and execute successfully.
6. `pay_without_a_covenant_claim_is_refused`: Asserts that calling `pay` without a covenant claim is explicitly rejected with `"familiar: spend without a covenant claim"`.

---

### 6. Documentation & Threat Model (`README.md`, `THREAT_MODEL.md`)

* **Privacy Claims vs. Code & Observer Output:**
  * Claims regarding budget balance confidentiality, unlinked single-use payments, and hidden recall leaf condition are accurate and verified by `docs/observer_view.md`.
  * **Imprecision on Recall Leaf Existence:** `THREAT_MODEL.md` lists "and that a recall path exists until it is used" under unrevealed items. When spending through the errand leaf, the Merkle proof publishes `siblings: 1`, revealing to an observer that the UTXO script tree has 2 leaves (though the recall leaf's content and condition remain hidden).
  * **Owner Key Visibility:** `Familiar::new` registers component access rules containing `rule!(public_key(owner))`. Consequently, the owner's public key is permanently published in cleartext in the component's state upon creation (see **Finding 4**).

---

## Findings

### Finding 1: Single-use payee root checks allow multiple outputs to the same root in a single `pay` transaction
* **Severity:** MEDIUM
* **File:Line:** `templates/familiar/src/lib.rs:114-124`, `151-153`
* **Concrete Scenario:** An agent constructs a `pay` transaction containing two separate stealth outputs, both locked under the same single-use payee root $R$ (for instance, splitting a payment of 200,000 µT into two 100,000 µT outputs to root $R$). During output iteration in `pay()`, `self.payees.contains(root)` is `true` for both outputs because `self.payees` is not modified until line 152 (`self.payees.retain(...)`) after the loop completes. The component accepts both outputs, allowing an agent to pay a single-use root multiple times in a single spend and linking those outputs on-chain.
* **Suggested Fix:** Track paid roots during output iteration and immediately disallow duplicate outputs to the same root when `single_use_payees` is enabled, or retain/remove used roots from `self.payees` as each output is processed.

```rust
// Suggested fix in templates/familiar/src/lib.rs:
SpendAuthorization::Script(root) if self.payees.contains(root) => {
    payee_outputs += 1;
    if self.single_use_payees && paid_roots.contains(root) {
        panic!("familiar: duplicate output to single-use payee root in single spend");
    }
    if !paid_roots.contains(root) {
        paid_roots.push(*root);
    }
}
```

---

### Finding 2: `observer_view.py` counts unexecuted payload UTXOs as hidden on-chain UTXOs for rejected transactions
* **Severity:** LOW
* **File:Line:** `scripts/observer_view.py:166-169`, `206-208`
* **Concrete Scenario:** For transactions where the main execution fails (such as Step 5a over-cap spend), `observe()` calls `describe_statement()` on the unexecuted `pay` arguments. `describe_statement()` appends input and output counts to `h`. `observe()` then sums all tuple entries in `h` into `n_out` and `n_in`. As a result, `docs/observer_view.md` reports "2 spent UTXO(s)" and "3 new UTXO(s)" under **Hidden** for Step 5a, even though the main instruction was rejected and those UTXOs were never created or spent on-chain.
* **Suggested Fix:** Accumulate input/output hidden counts only for accepted transactions (`outcome == "Commit"`), or separate unexecuted argument analysis from on-chain state diff summaries in `scripts/observer_view.py`.

---

### Finding 3: `agent.rs` parser fails on payment instructions where `to` contains punctuation
* **Severity:** LOW
* **File:Line:** `client/src/agent.rs:36`
* **Concrete Scenario:** In `parse()`, `unit` is cleaned using `name(unit) != "ut"`, which strips trailing punctuation. However, `to` is compared directly as `to != "to"`. If an inbox message contains `"pay 100 uT to, candle-shop"` or `"pay 100 uT to: candle-shop"`, `to` resolves to `"to,"` or `"to:"`, causing `to != "to"` to evaluate to `true` and `parse()` to return `None`.
* **Suggested Fix:** Normalize `to` using `name(to) != "to"` in `client/src/agent.rs`.

---

### Finding 4: Owner public key is explicitly published in cleartext in component access rules
* **Severity:** LOW
* **File:Line:** `templates/familiar/src/lib.rs:72-78`, `THREAT_MODEL.md:58`
* **Concrete Scenario:** `Familiar::new` registers component access rules referencing `rule!(public_key(owner))` for administrative methods (`pause`, `resume`, `set_policy`, etc.). Because component state and access rules are public on Ootle, the owner's public key is published in cleartext at component creation. `THREAT_MODEL.md` attributes key linking to funding keys from a single account in the demo, but `rule!(public_key(owner))` inherently leaks the owner's public key on-chain at creation time regardless of funding method.
* **Suggested Fix:** Update `THREAT_MODEL.md` to clarify that component access rules publish the owner public key on-chain upon component creation.

---

## Final Verdict

**VERDICT: APPROVED WITH FINDINGS** (1 MEDIUM, 3 LOW)
