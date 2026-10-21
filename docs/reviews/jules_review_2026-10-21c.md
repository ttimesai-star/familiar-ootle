# Code Review: Familiar Ootle (Cycle 3 / Commit 3030e8f)

**Date:** 2026-10-21  
**Reviewer:** Jules  
**Scope:** Cycle 3 changes (`CYCLE3_DIFF.patch`, `client/src/cli.rs`, `client/src/main.rs`, `tests/leash.rs`, `scripts/observer_view.py`, `scripts/make_page.py`, `scripts/make_tryit_gif.py`, `README.md`, `THREAT_MODEL.md`, `templates/familiar/src/lib.rs`).

---

## Detailed Checklist Verification

### 1. Recall Leaf Unpredictability & Disclosure Checks
- **Unpredictability:** The recall leaf of the budget and coupon condition trees is defined as `key_leaf(recall_keys.pk)` where `recall_keys` is a freshly generated random Ristretto key pair (`new_keys()`) owned exclusively by the owner client.
- **On-chain Exposure:** The public key (`recall_keys.pk`) is **not** passed in `Familiar::new` component arguments, `ComponentAccessRules`, `set_budget_root` arguments, `set_coupons` arguments, or any on-chain substate.
- **Off-chain Storage:** The secret key is saved locally in `me.json` (under `"recall"`) and stays off-chain.
- **Run Report & Indexer Check:** In `client/src/main.rs`, `recall_pk` is recorded in report JSON `ids` only at the end of execution (post-run). `scripts/observer_view.py` explicitly confirms that `recall_pk` appears in 0 transactions prior to the recall step and is absent from the leash's public state/access rules.

### 2. Co-signed Recall Authorization & Access Control
- **Authorization:** Spending through the recall leaf requires a valid signature from `recall_keys.sk`. In `client/src/main.rs` and `client/src/cli.rs`, `build_tx_cosigned` seals the transaction with `owner.keys.sk` (paying network fees from the owner's fee coin) and adds a co-signature from `recall_keys.sk`.
- **Access Control:** The agent key and external strangers do not possess `recall_keys.sk`.
- **Covenant Claim & Privacy:** The recall leaf carries no covenant (`BalancePreserved`), so the recall statement requires no covenant claim (`condition_root: None`). The amount recalled remains unrevealed on-chain.
- **Unit Tests:** `tests/leash.rs` verifies that:
  - `stranger_cannot_use_the_recall_leaf` is refused (`Access Denied: native.stealth_utxo.spend`).
  - `the_owner_key_alone_cannot_use_the_recall_leaf` is refused (`Access Denied: native.stealth_utxo.spend`).
  - `the_owner_key_alone_cannot_recall_coupons` is refused (`Access Denied: native.stealth_utxo.spend`).

### 3. Privacy of Coupon Denomination
- **Account Withdrawals:** `familiar leash` uses a fixed withdrawal amount (`LEASH_WITHDRAW` = 4,500,000 µT / 4.5 tTARI), decoupling account withdraws from coupon denomination.
- **On-chain Commitments & Fees:** `fund_coupons` creates hidden outputs with Pedersen commitments. Transaction fee intents reveal only the flat transaction fee (`FEE` = 100,000 µT) in cleartext (`revealed_in = 0`, `revealed_out = None`).
- **Reports & Artifacts:** In `client/src/main.rs`, `coupon_ut` is drawn at random (`random_coupon_ut()`) and explicitly excluded from Markdown reports and JSON outputs.
- **Denomination Range:** The denomination is known only to local wallet file `me.json`. The public codebase discloses only the sampling range (`COUPON_MIN_UT` = 100,000 µT to `COUPON_MAX_UT` = 124,999 µT).

### 4. Tap Tools, Coupon Bounds, and Withdraw Arithmetic
- **Tap Take & Withdraw Arithmetic:**
  - `TAP_TAKE` = 5,000,000 µT (5.0 tTARI); net after tap fee = 4,900,000 µT.
  - `LEASH_WITHDRAW` = 4,500,000 µT (4.5 tTARI) <= 4,900,000 µT. Fits in one tap take.
  - `AGENT_FEES` = 2,000,000 µT (20 * `FEE`); Withdraw Fee = 100,000 µT.
  - `owner_share` = 4,500,000 - 2,000,000 - 100,000 = 2,400,000 µT.
  - Max coupon value (`COUPON_LIMIT_UT`) = 140,000 µT; 10 coupons * 140,000 µT = 1,400,000 µT.
  - `OWNER_FEES` = 1,000,000 µT (10 * `FEE`).
  - `10 * COUPON_LIMIT_UT + OWNER_FEES` = 2,400,000 µT == `owner_share`. Arithmetic is exact.
- **`pick_coupon_ut` Bounds:** Validates `--coupon` input against `1..=COUPON_LIMIT_UT` (140,000 µT). Zero and values above 140,000 µT are rejected with informative error messages.
- **`tap_status` and `tap_top_up`:** `tap_status` correctly queries indexer endpoints and parses CBOR substates for vault balances and take counts. `tap_top_up` constructs valid fee and fund withdrawal instructions calling `tap.top_up`.

### 5. Documentation Alignment
- **Consistency:** `scripts/make_page.py` extracts the visible/hidden matrix directly from `README.md` (`privacy_table_html()`). `THREAT_MODEL.md` shares the exact same table structure and text.
- **Accuracy of Claims:** All claims regarding what is visible (transaction presence, epoch, leash address, agent signing key, network fee, coupon counts, payee roots, second leaf sibling hash) and hidden (µT amounts, coupon denomination, budget balance, payee identity behind roots, recall key before spend) accurately reflect the implementation.

---

## Findings

### HIGH
* **None.**

### MEDIUM
* **None.**

### LOW
* **None.**