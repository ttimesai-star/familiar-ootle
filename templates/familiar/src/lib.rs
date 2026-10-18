//! Familiar: a private, network-enforced allowance for AI agents on Tari Ootle.
//!
//! Spike stage (S1): the component only executes a stealth transfer from inside its own frame, so the engine tests
//! can check that a UTXO whose spend leaf requires `component(<this component>)` can be spent here and nowhere else.
//! The allowance logic (window, limit, payee allow-list) lands in S3.

use tari_template_lib::prelude::*;

#[template]
mod familiar {
    use super::*;

    pub struct Familiar {
        resource: ResourceAddress,
        revealed: Vault,
    }

    impl Familiar {
        pub fn new(resource: ResourceAddress) -> Component<Self> {
            Component::new(Self {
                resource,
                revealed: Vault::new_empty(resource),
            })
            .with_access_rules(AccessRules::allow_all())
            .create()
        }

        /// SPIKE ONLY: executes `statement` from inside this component. Any revealed output is kept here.
        pub fn spend_raw(&mut self, statement: StealthTransferStatement) {
            let manager = ResourceManager::get(self.resource);
            if let Some(bucket) = manager.stealth_transfer(statement) {
                self.revealed.deposit(bucket);
            }
        }
    }
}
