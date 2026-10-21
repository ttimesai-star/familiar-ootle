# Jules code review (sessions/15838809509421687569, snapshot review_snapshot_familiar)

# Code Review Report: Tari Ootle Familiar Snapshot

Below is the code review report covering the four requested focus areas across `templates/familiar/src/lib.rs`, `core/src/lib.rs`, `client/src/main.rs`, and `tests/leash.rs`.

---

### 1. `templates/familiar/src/lib.rs`: Incomplete Outflow Verification & Unchecked Claims in `pay()`

- **File and Function:** `templates/familiar/src/lib.rs` :: `Familiar::pay`
- **Concrete Input:** A caller submits a `StealthTransferStatement` containing:
  1. Multiple budget UTXO stealth inputs (or inputs spending multiple UTXOs), accompanied by multiple `CovenantClaim` items in `spend.covenant_claims`.
  2. `SpendAuthorization::Key` or `SpendAuthorization::KeyAndConditions` on payee outputs, or unvalidated conditions on output condition roots.
  3. Mismatched or inflated `covenant_claims` relative to actual cleartext payouts to allow-listed payee roots.
- **Expected vs Actual Behavior:**
  - *Expected:* The contract must verify that:
    - The charged amount matches the sum of cleartext amounts actually delivered to allow-listed payee roots.
    - Every input satisfies the budget root script (or is validated).
    - Every output strictly uses `SpendAuthorization::Script(root)` matching either `budget_root` or an allow-listed payee root.
    - Covenant claims map 1-to-1 to input covenant requirements rather than being an un-indexed sum.
  - *Actual:* 
    - `pay()` loops over `spend.covenant_claims` and sums `claim.revealed_amount` without verifying which input or output claim corresponds to which UTXO, or whether `covenant_claims.len()` matches `spend.stealth_inputs().len()`. If a transaction spends 2 budget UTXOs (each with a `BalancePreserved(400_000)` covenant), `spend.covenant_claims` could contain claims that do not correspond to the actual payee outputs or could double-count/under-count claims if the engine allows arbitrary claim vectors.
    - Outputs are validated only via `output.auth`. If an attacker constructs an output with `SpendAuthorization::KeyAndConditions` or another enum variant that wraps an allow-listed root, or if `output.auth` matching `budget_root` is used as payout (or vice-versa), `payee_outputs` is incremented without checking that the amount assigned to `budget_root` is true change vs payee allocation.
- **Minimal Code Change:**
  ```rust
  // In Familiar::pay():
  // 1. Ensure outputs strictly use Script(root) and count payee outflow explicitly:
  let mut payee_outputs = 0u64;
  for output in spend.stealth_outputs() {
      match &output.auth {
          SpendAuthorization::Script(root) if *root == budget_root => {},
          SpendAuthorization::Script(root) if self.payees.contains(root) => payee_outputs += 1,
          _ => panic!("familiar: output to a payee not on the leash"),
      }
  }
  assert!(payee_outputs > 0, "familiar: no payee output");

  // 2. Cross-check covenant claims count against inputs count:
  assert_eq!(
      spend.covenant_claims.len(),
      spend.stealth_inputs().len(),
      "familiar: covenant claims count mismatch"
  );
  ```
- **Confidence Level:** 92%

---

### 2. `core/src/lib.rs`: Window Arithmetic Boundaries and Dynamic `set_policy()` Epoch Updates

- **File and Function:** `core/src/lib.rs` :: `current()`, `charge()`, and `templates/familiar/src/lib.rs` :: `set_policy()`
- **Concrete Input:** 
  1. *Boundary condition:* At epoch `now = window.start + policy.window_epochs`.
  2. *Policy change:* `set_policy(new_limit, new_window_epochs)` is called at epoch `now` where `now - window_start < old_window_epochs`, but `now - window_start >= new_window_epochs`.
- **Expected vs Actual Behavior:**
  - *Boundary condition:*
    - `elapsed = now - window.start = policy.window_epochs`.
    - `elapsed < policy.window_epochs` is `false`.
    - `elapsed % policy.window_epochs = 0`.
    - `new_start = now - 0 = now`. `spent` is reset to `0`.
    - *Behavior:* Correct. Epoch `window.start + window_epochs` starts a new window aligned to `now`.
  - *Policy change (`set_policy`):*
    - *Expected:* When `set_policy()` updates `window_epochs`, the stored `window_start` and `spent` should either be re-aligned/evaluated against the new policy immediately or rolled forward using `current()`, or documented as retaining `spent` until the next `charge()`.
    - *Actual:* `set_policy()` in `templates/familiar/src/lib.rs` only updates `self.limit` and `self.window_epochs` without advancing `self.window_start` or resetting/adjusting `self.spent`. If `new_window_epochs` is much smaller than `old_window_epochs`, a subsequent call to `charge()` at epoch `now` will compute `elapsed = now - self.window_start` using `new_window_epochs`, causing `now - elapsed % new_window_epochs` to jump to a historical grid point within the old window while preserving `self.spent`. If `self.spent` was already higher than `new_limit`, the next payment will be refused until a full `new_window_epochs` passes from the adjusted start.
- **Minimal Code Change:**
  ```rust
  // In templates/familiar/src/lib.rs :: set_policy():
  pub fn set_policy(&mut self, limit: u64, window_epochs: u64) {
      let policy = Policy { limit, window_epochs };
      if let Err(refusal) = policy.validate() {
          panic!("{refusal}");
      }
      let current_window = Window { start: self.window_start, spent: self.spent };
      let now = Consensus::current_epoch();
      // Roll window state to current epoch before updating policy parameters
      if let Ok(updated) = familiar_core::current(&Policy { limit: self.limit, window_epochs: self.window_epochs }, current_window, now) {
          self.window_start = updated.start;
          self.spent = updated.spent;
      }
      self.limit = limit;
      self.window_epochs = window_epochs;
  }
  ```
- **Confidence Level:** 95%

---

### 3. `client/src/main.rs`: Risk of Secret Key Exposure in Error Logs & Markdown Reports

- **File and Function:** `client/src/main.rs` :: `reason_of()`, `funder_secret()`, and `main()`
- **Concrete Input:** 
  1. An error occurs during transaction preparation, dry-run, or claim parsing where exception/debug strings formatted with `{:?}` contain secret key struct representations (e.g. `OotleSecretKey` or `RistrettoSecretKey`).
  2. `FAMILIAR_FUNDER` env var is misformatted or invalid.
- **Expected vs Actual Behavior:**
  - *Expected:* Secret keys must never be formatted or included in `anyhow!` error context chains or transaction debug logs that get captured by `reason_of()` and appended to `r.md` (the report file) or printed to `stdout`.
  - *Actual:*
    - In `reason_of(debug: &str)`: `dry` outputs and execution failures are formatted via `format!("{:?}", r.finalize.result)`. If an engine error or transaction object stringifies internal witnesses/keys (e.g. `OotleSecretKey` or input secret keys in error messages), `reason_of()` slices up to 700 characters of the tail/debug string and writes it directly into `r.md` via `r.row(...)`.
    - In `funder_secret()`: Parsing errors return `anyhow!("FAMILIAR_FUNDER must be...")` or `anyhow!("account secret: {e:?}")`. While `RistrettoSecretKey` hex parsing errors typically don't mirror the key, raw string formatting of error context in helper functions risks leaking environment variable content if debugged.
- **Minimal Code Change:**
  ```rust
  // In client/src/main.rs :: reason_of():
  fn reason_of(debug: &str) -> String {
      // Sanitize debug string to strip hex patterns or secret key representations before logging to report
      let sanitized = debug.replace(|c: char| c.is_control(), " ");
      let clean = if let Some(i) = sanitized.find("message: \"") {
          let rest = &sanitized[i + 10..];
          if let Some(j) = rest.find('"') {
              one_line(&rest[..j])
          } else {
              one_line(rest)
          }
      } else {
          one_line(&sanitized)
      };
      // Prevent key leaks if debug strings contain secret key representations
      clean.replace("SecretKey", "[REDACTED_KEY]")
  }
  ```
- **Confidence Level:** 88%

---

### 4. `tests/leash.rs`: Missing Critical Negative Test Cases

- **File and Function:** `tests/leash.rs`
- **Concrete Input:** The test suite is missing negative tests for the following edge cases:
  1. **Multiple Budget Inputs in a Single Payment:** No test verifies `pay()` behavior when spending multiple budget UTXOs in one transaction or when multiple `covenant_claims` are present.
  2. **Unset Budget Root Payment Attempt Before `set_budget_root`:** Tested in `pay_without_a_budget_root_is_refused`, but no test checks attempting to call `pay` after `set_payees` or `set_policy` is modified prior to setting `budget_root`.
  3. **Zero Amount Payment / Negative Allowance Charge:** No test verifies `pay()` rejection when `covenant_claims` sum to `0` (which triggers `Refusal::ZeroPayment` in `familiar_core::charge`).
  4. **`set_policy` Epoch Change In The Middle of an Active Window:** No test checks `set_policy` reducing `window_epochs` while `spent > 0` in an active window.
  5. **Duplicate Payee in `set_payees`:** No test verifies that `set_payees` rejects duplicates (unlike `new` which is tested).
  6. **Payee Output with Non-Script Auth (e.g., KeyPath to Payee):** No test verifies rejection when an output pays an allow-listed payee using `OutputAuthSpec::KeyPath` instead of `OutputAuthSpec::Conditions`.
- **Expected vs Actual Behavior:**
  - *Expected:* Comprehensive test suite covering all contract rejection conditions and state mutation edge cases.
  - *Actual:* Missing coverage for multi-UTXO claims, zero-amount spend attempts, mid-window `set_policy` epoch adjustments, and `set_payees` duplicate checks.
- **Minimal Code Change:**
  ```rust
  // Add in tests/leash.rs:

  #[test]
  fn zero_amount_payment_is_refused() {
      let mut w = world();
      // Generate transfer data with 0 outflow claim
      let data = pay_statement(&w, 0, 0, payee_auth(&w, 0), budget_auth(&w));
      pay_refused(&mut w, &data, Who::Agent, "familiar: zero payment");
  }

  #[test]
  fn set_payees_rejects_duplicates_and_empty() {
      let mut w = world();
      let payee = root_of(&[key_leaf(w.payees[0].pk)]);
      let tx = tx_method(&w, w.leash, "set_payees", args![vec![payee, payee]], &w.owner.sk.clone());
      let reason = w.test.execute_expect_failure(tx, vec![]);
      assert_reject_reason(&reason, "familiar: duplicate payee");
  }
  ```
- **Confidence Level:** 95%


---

I have completed the code review report for the Tari Ootle Familiar snapshot codebase. The review covers:

1. **`templates/familiar/src/lib.rs`**: Verification of `pay()` input shapes, covenant claim bounds, and output authorization checks.
2. **`core/src/lib.rs`**: Correctness of window calculations at epoch boundaries and dynamic policy epoch adjustments in `set_policy()`.
3. **`client/src/main.rs`**: Key material leak risks in error handling, debug formatting, and markdown report generation.
4. **`tests/leash.rs`**: Missing negative test cases for multi-input transactions, zero-amount spends, dynamic policy changes, and duplicate payees in `set_payees`.

No files were modified and no pull request was created.