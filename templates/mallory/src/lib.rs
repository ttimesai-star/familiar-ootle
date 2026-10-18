//! Test-only hostile component. It executes any stealth transfer from its own frame, which is what an attacker who
//! stole the agent key would deploy to spend the leash's UTXOs outside the leash. Never published.

use tari_template_lib::prelude::*;

#[template]
mod mallory {
    use super::*;

    pub struct Mallory {
        resource: ResourceAddress,
        loot: Vault,
    }

    impl Mallory {
        pub fn new(resource: ResourceAddress) -> Component<Self> {
            Component::new(Self {
                resource,
                loot: Vault::new_empty(resource),
            })
            .with_access_rules(AccessRules::allow_all())
            .create()
        }

        pub fn steal(&mut self, statement: StealthTransferStatement) {
            if let Some(bucket) = ResourceManager::get(self.resource).stealth_transfer(statement) {
                self.loot.deposit(bucket);
            }
        }
    }
}
