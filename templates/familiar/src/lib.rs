//! Familiar: a private, network-enforced allowance for AI agents on Tari Ootle.
//!
//! The agent's budget is not a public vault. It is a set of stealth UTXOs (amounts hidden by Pedersen commitments)
//! locked under a pure condition tree with two leaves:
//!
//! * errand: `component(<this leash>)` AND `BalancePreserved(max_per_payment)`: the UTXOs can only be spent from
//!   inside this component, every hidden unit not paid out must go back under the same tree, and a single spend may
//!   move at most `max_per_payment` out;
//! * recall: `public_key(<owner one-time key>)`: the owner can sweep everything at any time. The leaf stays hidden
//!   until it is used.
//!
//! This component adds what a stateless condition cannot: a limit per window of epochs, an allow-list of payees and a
//! pause switch. `pay` refuses any spend that is not shaped like "leash UTXOs in, change back under the budget root,
//! the rest to allow-listed payees, nothing revealed".
//!
//! The covenant counts per condition root: all inputs under one root are one partition with one claim. When the
//! owner rotates the budget root, UTXOs under the old and the new root are two partitions, so a single spend could
//! move `max_per_payment` from each. The component therefore also caps the total claimed outflow of a spend
//! (found in review, see THREAT_MODEL.md).

use familiar_core::{Policy, Window};
use tari_template_lib::prelude::*;
use tari_template_lib::types::{Hash32, stealth::SpendAuthorization};

/// Upper bound on the allow-list, so the component state (and the cost of a `pay`) stays bounded.
pub const MAX_PAYEES: usize = 16;

#[template]
mod familiar {
    use super::*;

    pub struct Familiar {
        resource: ResourceAddress,
        limit: u64,
        window_epochs: u64,
        window_start: u64,
        spent: u64,
        budget_root: Option<Hash32>,
        /// The per-spend cap, the same number as in the errand leaf's `BalancePreserved`. Set with the budget root.
        max_per_payment: u64,
        payees: Vec<Hash32>,
        payments: u64,
        paused: bool,
    }

    impl Familiar {
        /// Creates a leash. `agent` and `owner` should be one-time keys: they authorise calls (as transaction
        /// signers) and are not tied to any account. The budget root is set afterwards with `set_budget_root`,
        /// because the errand leaf commits to this component's address, which only exists once it is created.
        pub fn new(
            resource: ResourceAddress,
            agent: RistrettoPublicKeyBytes,
            owner: RistrettoPublicKeyBytes,
            limit: u64,
            window_epochs: u64,
            payees: Vec<Hash32>,
        ) -> Component<Self> {
            let policy = Policy { limit, window_epochs };
            if let Err(refusal) = policy.validate() {
                panic!("{refusal}");
            }
            check_payees(&payees);
            assert!(agent != owner, "familiar: agent and owner keys must differ");

            Component::new(Self {
                resource,
                limit,
                window_epochs,
                window_start: Consensus::current_epoch(),
                spent: 0,
                budget_root: None,
                max_per_payment: 0,
                payees,
                payments: 0,
                paused: false,
            })
            .with_access_rules(
                ComponentAccessRules::new()
                    .method("pay", rule!(public_key(agent)))
                    .method("status", rule!(allow_all))
                    .method("pause", rule!(public_key(owner)))
                    .method("resume", rule!(public_key(owner)))
                    .method("set_policy", rule!(public_key(owner)))
                    .method("set_payees", rule!(public_key(owner)))
                    .method("set_budget_root", rule!(public_key(owner)))
                    .default(rule!(deny_all)),
            )
            // No component owner: an owner would bypass the method rules above (it could call `pay`), and nobody needs
            // to rewrite these rules. The owner key acts only through the owner-only methods and the recall leaf.
            .with_owner_rule(OwnerRule::None)
            .create()
        }

        /// The agent pays. `spend` must spend leash UTXOs through the errand leaf, return the change under the
        /// budget root and send the rest to allow-listed payees as stealth outputs. The amount charged against the
        /// window is the cleartext outflow the covenant itself proves (`covenant_claims`), never an argument.
        pub fn pay(&mut self, spend: StealthTransferStatement) {
            assert!(!self.paused, "familiar: leash is paused by the owner");
            let budget_root = self.budget_root.expect("familiar: budget root not set");

            assert!(
                spend.inputs_statement.revealed_amount.is_zero(),
                "familiar: revealed inputs are not allowed"
            );
            assert!(
                spend.outputs_statement.revealed_output.is_none(),
                "familiar: revealed outputs are not allowed"
            );
            assert!(!spend.stealth_inputs().is_empty(), "familiar: nothing spent");

            let mut payee_outputs = 0u64;
            for output in spend.stealth_outputs() {
                match &output.auth {
                    SpendAuthorization::Script(root) if *root == budget_root => {}
                    SpendAuthorization::Script(root) if self.payees.contains(root) => payee_outputs += 1,
                    _ => panic!("familiar: output to a payee not on the leash"),
                }
            }
            assert!(payee_outputs > 0, "familiar: no payee output");

            let mut amount = Amount::zero();
            for claim in &spend.covenant_claims {
                amount = amount
                    .checked_add(claim.revealed_amount)
                    .expect("familiar: arithmetic overflow");
            }
            let amount = amount.to_u64_checked().expect("familiar: amount out of range");

            let policy = Policy {
                limit: self.limit,
                window_epochs: self.window_epochs,
            };
            let window = Window {
                start: self.window_start,
                spent: self.spent,
            };
            let window = match familiar_core::charge(&policy, window, Consensus::current_epoch(), amount) {
                Ok(window) => window,
                Err(refusal) => panic!("{refusal}"),
            };

            let revealed = ResourceManager::get(self.resource).stealth_transfer(spend);
            assert!(revealed.is_none(), "familiar: unexpected revealed output");
            // Checked after the transfer on purpose: the covenant in the UTXO is the first line, so a single
            // over-cap spend is refused by the engine itself. This line only matters when the claims come from
            // several partitions (UTXOs under an old and a new budget root): each is within its own cap, their sum
            // is not.
            assert!(
                amount <= self.max_per_payment,
                "familiar: payment of {amount} over the per-payment cap {cap}",
                cap = self.max_per_payment
            );

            self.window_start = window.start;
            self.spent = window.spent;
            self.payments += 1;
            emit_event(
                "Paid",
                metadata![
                    "payment" => self.payments.to_string(),
                    "amount" => amount.to_string(),
                    "payee_outputs" => payee_outputs.to_string(),
                ],
            );
        }

        /// Public view of the leash: the rule and the current window. The budget itself is not here: it lives in
        /// hidden UTXOs that only the owner and the agent can open.
        pub fn status(&self) -> LeashStatus {
            let policy = Policy {
                limit: self.limit,
                window_epochs: self.window_epochs,
            };
            let window = Window {
                start: self.window_start,
                spent: self.spent,
            };
            let now = Consensus::current_epoch();
            let seen = familiar_core::current(&policy, window, now).unwrap_or(window);
            LeashStatus {
                limit: self.limit,
                window_epochs: self.window_epochs,
                max_per_payment: self.max_per_payment,
                window_start: seen.start,
                spent_in_window: seen.spent,
                remaining_in_window: self.limit.saturating_sub(seen.spent),
                payments: self.payments,
                paused: self.paused,
                budget_root_set: self.budget_root.is_some(),
            }
        }

        pub fn pause(&mut self) {
            self.paused = true;
            emit_event("Paused", metadata!["paused" => "true"]);
        }

        pub fn resume(&mut self) {
            self.paused = false;
            emit_event("Resumed", metadata!["paused" => "false"]);
        }

        /// Changes the rule. The window restarts at this epoch and keeps what was already spent in the window seen
        /// under the old rule, so a shorter window can never free what was spent (if `spent` is already over the new
        /// `limit`, the agent waits for the next window). Before this fix the grid was re-aligned in place and a
        /// shorter window could land on a past grid point, which reset `spent` (found in review).
        pub fn set_policy(&mut self, limit: u64, window_epochs: u64) {
            let policy = Policy { limit, window_epochs };
            if let Err(refusal) = policy.validate() {
                panic!("{refusal}");
            }
            let old = Policy {
                limit: self.limit,
                window_epochs: self.window_epochs,
            };
            let window = Window {
                start: self.window_start,
                spent: self.spent,
            };
            let window = match familiar_core::restart_window(&old, window, Consensus::current_epoch()) {
                Ok(window) => window,
                Err(refusal) => panic!("{refusal}"),
            };
            self.window_start = window.start;
            self.spent = window.spent;
            self.limit = limit;
            self.window_epochs = window_epochs;
        }

        pub fn set_payees(&mut self, payees: Vec<Hash32>) {
            check_payees(&payees);
            if let Some(root) = self.budget_root {
                assert!(!payees.contains(&root), "familiar: budget root cannot be a payee");
            }
            self.payees = payees;
        }

        /// Sets the condition root the budget UTXOs are locked under (computed by the owner's client from the
        /// errand and recall leaves) and the per-spend cap written into the errand leaf. UTXOs under a previous root
        /// stay spendable through `pay` (their change moves under the new root); the cap bounds the total outflow of a
        /// spend whatever roots its inputs are under.
        pub fn set_budget_root(&mut self, root: Hash32, max_per_payment: u64) {
            assert!(!self.payees.contains(&root), "familiar: budget root cannot be a payee");
            assert!(max_per_payment > 0, "familiar: per-payment cap must be positive");
            self.budget_root = Some(root);
            self.max_per_payment = max_per_payment;
        }
    }

    pub struct LeashStatus {
        pub limit: u64,
        pub window_epochs: u64,
        pub max_per_payment: u64,
        pub window_start: u64,
        pub spent_in_window: u64,
        pub remaining_in_window: u64,
        pub payments: u64,
        pub paused: bool,
        pub budget_root_set: bool,
    }

    fn check_payees(payees: &[Hash32]) {
        assert!(!payees.is_empty(), "familiar: at least one payee is required");
        assert!(payees.len() <= MAX_PAYEES, "familiar: too many payees");
        for (i, payee) in payees.iter().enumerate() {
            assert!(!payees[..i].contains(payee), "familiar: duplicate payee");
        }
    }
}
