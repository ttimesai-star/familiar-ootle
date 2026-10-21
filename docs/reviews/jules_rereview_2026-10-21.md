# Jules re-review 21.10 (session 5797070406684270828), extracted from its patch

# Re-Review Report: Familiar Snapshot (`review_snapshot_familiar`)
Date: 2026-10-21

This is a read-only re-review of `projects/contests/tari_ootle/review_snapshot_familiar` evaluating the fixes applied for previous review findings (F1–F4) and checking for any newly introduced issues.

---

## 1. Evaluation of Previous Findings & Fixes

### (F1 MEDIUM) Two outputs to one single-use payee root in one spend
* **Fix Location:** `templates/familiar/src/lib.rs` (lines 106–118)
* **Assessment:** **Correct and Complete.**
  When iterating through `spend.stealth_outputs()`, `paid_roots` tracks all payee roots seen within the single spend statement. If a payee root appears more than once and `single_use_payees` is enabled, the component panics with `"familiar: single-use payee root paid twice in one spend"`. After verification, all `paid_roots` are retained out of `self.payees`.
* **Test Verification:**
  - **Test:** `tests/leash.rs::single_use_root_takes_one_output_even_within_one_spend`
  - **Status:** Present and correct. It tests that attempting to send two outputs to the same single-use payee root in one spend fails when `single_use_payees` is active, and succeeds when `single_use_payees` is turned off.

---

### (F2 LOW) `observer_view.py` counted UTXOs of rejected instructions as hidden
* **Fix Location:** `scripts/observer_view.py` (lines 185–189, 206–208)
* **Assessment:** **Correct and Complete.**
  `observer_view.py` checks whether a transaction outcome was rejected (`summary.get("outcome") != "Commit"`). If rejected, it strips any input/output counts recorded for the main instruction body, ensuring only the fee intent's UTXOs are counted as hidden state on-chain.
* **Test Verification:** N/A (Script / evidence generation tool fix verified by inspection).

---

### (F3 LOW) `agent.rs` parser compared `'to'` without normalisation
* **Fix Location:** `client/src/agent.rs` (lines 19–23, 41)
* **Assessment:** **Correct and Complete.**
  The `name()` helper function strips non-alphanumeric trailing/leading characters (preserving hyphens and underscores). Line 41 uses `name(to) != "to"`, allowing the parser to tolerate punctuation attached to "to" (e.g. `to:` or `to,`).
* **Test Verification:**
  - **Test:** `client/src/agent.rs::tolerates_punctuation_after_to`
  - **Status:** Present and correct. Asserts parsing succeeds for strings containing `to:` and `to,`.

---

### (F4 LOW) Docs overclaimed that the recall path's existence is hidden
* **Fix Location:** `README.md` and `THREAT_MODEL.md`
* **Assessment:** **Correct and Complete.**
  Documentation was updated to clarify that while the *content* and meaning of the recall leaf remain hidden until spent, the *existence* of a second leaf in the budget UTXO condition tree is visible on-chain because each payment reveals one Merkle sibling hash.

---

## 2. New Issues Audit

No new security, logic, or privacy issues were introduced across the modified code, scripts, tests, or documentation.

---

## 3. Verdict

**VERDICT: APPROVED** — All identified findings (F1–F4) have been correctly and completely resolved with appropriate test coverage, and no new issues were introduced.
