//! A tTARI tap for trying Familiar on the Esmeralda testnet. The shared faucet is often empty, so the author keeps this
//! one topped up: anyone can take `per_take` (enough for a leash with coupons and a few payments), at most
//! `per_epoch` times per epoch. Testnet only; tTARI has no value.

use tari_template_lib::prelude::*;

#[template]
mod familiar_tap {
    use super::*;

    pub struct FamiliarTap {
        funds: Vault,
        per_take: Amount,
        per_epoch: u64,
        epoch: u64,
        taken_in_epoch: u64,
        takes: u64,
    }

    impl FamiliarTap {
        pub fn new(funds: Bucket, per_take: Amount, per_epoch: u64) -> Component<Self> {
            assert!(per_take.is_positive(), "tap: per_take must be positive");
            assert!(per_epoch > 0, "tap: per_epoch must be positive");
            Component::new(Self {
                funds: Vault::from_bucket(funds),
                per_take,
                per_epoch,
                epoch: Consensus::current_epoch(),
                taken_in_epoch: 0,
                takes: 0,
            })
            .with_access_rules(AccessRules::allow_all())
            // Nobody can change the rules or take the vault out: the tap only gives `per_take` at a time.
            .with_owner_rule(OwnerRule::None)
            .create()
        }

        pub fn take(&mut self) -> Bucket {
            let now = Consensus::current_epoch();
            if now != self.epoch {
                self.epoch = now;
                self.taken_in_epoch = 0;
            }
            assert!(
                self.taken_in_epoch < self.per_epoch,
                "tap: {} takes this epoch already, come back next epoch",
                self.per_epoch
            );
            assert!(
                self.funds.balance() >= self.per_take,
                "tap: empty, ask the author to top it up"
            );
            self.taken_in_epoch += 1;
            self.takes += 1;
            self.funds.withdraw(self.per_take)
        }

        pub fn top_up(&mut self, funds: Bucket) {
            self.funds.deposit(funds);
        }

        pub fn left(&self) -> Amount {
            self.funds.balance()
        }
    }
}
