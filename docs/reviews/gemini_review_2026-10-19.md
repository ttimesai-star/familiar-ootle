model: gemini-3.8-flash

Here is the security review of the project against the specified threat model.

---

### 1. [HIGH] Allowance Bypass: Multiple Inputs Evade the Covenant `BalancePreserved` per Spend
* **File & Function:** `templates/familiar/src/lib.rs` :: `pay`
* **Scenario:**
  1. The attacker controls the agent key and has the secrets for multiple budget UTXOs (e.g., two UTXOs of 100 µT each, where `max_per_payment = 40`).
  2. The attacker constructs a single `StealthTransferStatement` containing two stealth inputs (both budget UTXOs) and one payee output of 80 µT (plus change back to `budget_root`).
  3. `BalancePreserved(40)` is evaluated by the engine per UTXO spend/script path or claim. Under native covenant semantics, each input UTXO permits up to 40 µT outflow (or generates its own claim). The engine permits a total revealed outflow across the transaction equal to the sum of claims ($\sum 40 = 80$).
  4. In `familiar::pay`:
     ```rust
     let mut amount = Amount::zero();
     for claim in &spend.covenant_claims {
         amount = amount.checked_add(claim.revealed_amount)...;
     }
     ```
     `spend.covenant_claims` contains claims summing to 80 µT. If the window limit allows 80 µT, `familiar_core::charge` succeeds because it only checks `amount <= limit`.
  5. The attacker successfully moves 80 µT in a single payment transaction, violating the primary security invariant: *"never more than `max_per_payment` per spend"*.
* **Why existing checks do not stop it:** `pay` verifies `!spend.stealth_inputs().is_empty()` and checks that payee outputs are allow-listed, but does not assert `spend.stealth_inputs().len() == 1` nor does it check that `amount <= max_per_payment` (or `claim.revealed_amount <= max_per_payment` per spend).
* **Fix:** If the design requires that each payment spend consumes at most one budget UTXO or caps the spend size, enforce `assert_eq!(spend.stealth_inputs().len(), 1, "familiar: multi-input spend not allowed");` in `pay`, or store `max_per_payment` on the `Familiar` component and assert `assert!(amount <= self.max_per_payment)`.

---

### 2. [HIGH] Missing Verification of Covenant Resource and Claim Association in `pay`
* **File & Function:** `templates/familiar/src/lib.rs` :: `pay`
* **Scenario:**
  1. `covenant_claims` in a `StealthTransferStatement` is a vector of claimed outflows declared on the statement.
  2. An attacker crafts a statement where `covenant_claims` is empty (`spend.covenant_claims.is_empty()`), or references another token/resource or dummy claim, while moving funds from the budget UTXOs into a payee output.
  3. If the covenant on the errand leaf only enforces constraints when a claim is processed against the covenant, or if `covenant_claims` can be manipulated without matching the actual value moved between confidential commitments (since amounts are hidden Pedersen commitments):
     `amount` evaluates to `0`. `charge(&policy, window, now, 0)` then fails with `Refusal::ZeroPayment`.
  4. Conversely, if the attacker claims `claim.revealed_amount = 1` while moving 40 µT into a payee commitment (assuming the engine allows confidential transfer balancing where input commitment = change commitment + payee commitment without full disclosure of individual stealth output amounts), `pay` only charges `1 µT` against the window limit.
  5. The agent drains the entire budget far exceeding `limit` per window.
* **Why existing checks do not stop it:** `pay` completely trusts `spend.covenant_claims` to derive `amount` without verifying that the covenant claim is non-empty, matches the resource being transferred, or covers the actual outflow to the payee.
* **Fix:** Enforce `assert!(!spend.covenant_claims.is_empty(), "familiar: missing covenant claim")`. Ensure the claim explicitly targets `self.resource`.

---

### 3. [MEDIUM] `set_policy` Allows Window Shift and Quota Reset Exploitation
* **File & Function:** `templates/familiar/src/lib.rs` :: `set_policy` & `core/src/lib.rs` :: `current`
* **Scenario:**
  1. The owner updates the policy via `set_policy(new_limit, new_window_epochs)`.
  2. In `set_policy`, `self.limit` and `self.window_epochs` are modified directly, but `self.window_start` is left untouched.
  3. `familiar_core::current` computes:
     ```rust
     let elapsed = now - window.start;
     let new_start = now - elapsed % policy.window_epochs;
     ```
  4. If `window_epochs` is changed while a window is active, `elapsed % policy.window_epochs` alters the alignment grid retroactively. Depending on `now` and `elapsed`, `elapsed < policy.window_epochs` may suddenly evaluate to false even if the old window was not over, resetting `window.spent` immediately to `0`.
  5. The agent can immediately spend the full limit again before the expected window duration has elapsed.
* **Why existing checks do not stop it:** `set_policy` only executes `policy.validate()`, ignoring the current window state and elapsed time.
* **Fix:** Roll the current window forward using the old policy before updating to the new policy, or update `window_start` to `now` and preserve/reset `spent` consistently:
  ```rust
  pub fn set_policy(&mut self, limit: u64, window_epochs: u64) {
      let now = Consensus::current_epoch();
      let status = self.status();
      self.window_start = status.window_start;
      self.spent = status.spent_in_window;
      self.limit = limit;
      self.window_epochs = window_epochs;
  }
  ```

---

### 4. [MEDIUM] Client Secret Key Exposure via CI Artifacts on Failure
* **File & Function:** `.github/workflows/esmeralda.yml` :: `scenario` step & `client/src/main.rs`
* **Scenario:**
  1. The workflow runs on `workflow_dispatch` with secret `FAMILIAR_FUNDER` passed in the environment.
  2. In `.github/workflows/esmeralda.yml`:
     ```yaml
     run: cargo run -p familiar-cli --release -- scenario familiar.min.wasm esmeralda_run.md 2>&1 | tee scenario.log
     ...
     - name: upload report
       if: always()
       uses: actions/upload-artifact@v4
       with:
         name: esmeralda-report
         path: |
           esmeralda_run.md
           scenario.log
     ```
  3. In `client/src/main.rs`:
     During execution, functions such as `funder_secret` return errors containing context:
     `funder_from` / `funder_secret` (e.g. `FAMILIAR_FUNDER must be <account hex>:<view hex>`), or if any JSON-RPC request/response dumps the request headers/parameters on indexer failure, or if an unexpected `panic!` or error prints `std::env::var("FAMILIAR_FUNDER")`.
  4. `scenario.log` is uploaded with `if: always()` as an unencrypted public GitHub artifact, exposing `FAMILIAR_FUNDER` (the funding account's private keys) to anyone with read access to the repository.
* **Why existing checks do not stop it:** `scenario.log` captures `stderr` (`2>&1`) indiscriminately and uploads it unconditionally (`if: always()`).
* **Fix:** Avoid capturing full verbose CLI logs into public artifacts, or sanitize secrets from logs before upload. Ensure error messages never echo environment values.

---

### 5. [LOW] Arithmetic Truncation / Precision Mismatch between `Amount` and `u64`
* **File & Function:** `templates/familiar/src/lib.rs` :: `pay`
* **Scenario:**
  1. `Amount` in `tari_template_lib` represents an integer or high-precision value depending on token denomination.
  2. In `pay`:
     ```rust
     let amount = amount.to_u64_checked().expect("familiar: amount out of range");
     ```
  3. If a custom resource token uses decimal scales or values exceeding $2^{64}-1$ base units, `to_u64_checked()` returns `None`, permanently bricking `pay()` transactions for valid transfer statements that would otherwise be supported by the native confidential engine.
* **Why existing checks do not stop it:** `Familiar` stores `limit`, `spent`, and `max_per_payment` as `u64`, forcing conversion from `Amount` to `u64`.
* **Fix:** Store amounts as `Amount` across `Policy`, `Window`, and the template state rather than casting to primitive `u64`.

---

### 6. [LOW] UNCERTAIN: Agent Key Leakage via Transaction Fee Intent
* **File & Function:** `client/src/main.rs` :: `fee_statement` / `build_tx`
* **Scenario:**
  1. The threat model notes: *"The agent's fee coin is a key-path UTXO of the agent key. Fund fees from fresh stealth outputs."*
  2. In `client/src/main.rs`, the same `agent.keys` used to authorize `pay` is also used to sign fee instructions:
     ```rust
     fee_statement(&actor.fee_coin, &actor.keys, fee)
     ```
  3. Because the fee coin uses `SpendAuthorization::Key(who.pk)` and reveals `fee` to `who.pk`, the agent's public key is revealed on-chain with every spend.
  4. If an attacker compromises the agent key, they can not only spend allowance within limits, but also front-run or exhaust the fee coin directly through plain L2 transactions, causing denial of service on legitimate agent operations.
* **Why existing checks do not stop it:** Fee coins use key paths directly instead of one-time stealth authorizations.
* **Fix:** Use one-time unlinkable stealth keys for fee payments separate from the agent's component invocation authority.