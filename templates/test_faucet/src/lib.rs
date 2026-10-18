//! Test-only faucet: creates a fresh stealth resource, keeps its supply as revealed funds, and turns part of it into
//! UTXOs under any spend authorisation a client-built statement describes. Used by the engine tests to put stealth
//! funds under Familiar's condition tree. Never published.

use tari_template_lib::prelude::*;

#[template]
mod test_faucet {
    use super::*;

    pub struct TestFaucet {
        supply: Vault,
    }

    impl TestFaucet {
        /// Mints `initial_supply` of a new stealth resource. `mint` must reveal the whole supply back as a revealed
        /// output (no stealth outputs), which is kept here as the faucet's supply.
        pub fn new(initial_supply: Amount, mint: StealthTransferStatement) -> Component<Self> {
            let bucket = ResourceBuilder::stealth()
                .with_token_symbol("tFAM")
                .initial_supply(initial_supply);
            let revealed = bucket.stealth_transfer(mint);
            Component::new(Self {
                supply: Vault::from_bucket(revealed),
            })
            .with_access_rules(AccessRules::allow_all())
            .create()
        }

        pub fn resource_address(&self) -> ResourceAddress {
            self.supply.resource_address()
        }

        /// Spends `transfer.inputs_statement.revealed_amount` of the supply into the statement's stealth outputs.
        pub fn fund(&mut self, transfer: StealthTransferStatement) {
            let input = self.supply.withdraw(transfer.inputs_statement.revealed_amount);
            let manager = ResourceManager::get(self.supply.resource_address());
            if let Some(change) = manager.stealth_transfer_with_opt_input_bucket(transfer, Some(input)) {
                self.supply.deposit(change);
            }
        }
    }
}
