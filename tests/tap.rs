//! Engine tests for the tTARI tap (templates/tap), the helper that lets anyone try Familiar on Esmeralda.

use tari_template_lib::types::{Amount, ComponentAddress, constants::XTR};
use tari_template_test_tooling::{
    TemplateTest,
    crypto::RistrettoSecretKey,
    engine_types::virtual_substate::{VirtualSubstate, VirtualSubstateId},
    support::assert_error::assert_reject_reason,
    transaction::{Transaction, args},
};

const PER_TAKE: u64 = 100;

fn tap_world(funds: u64) -> (TemplateTest, ComponentAddress, ComponentAddress, RistrettoSecretKey) {
    let mut test = TemplateTest::new(env!("CARGO_MANIFEST_DIR"), ["templates/tap"]);
    test.set_virtual_substate(VirtualSubstateId::CurrentEpoch, VirtualSubstate::CurrentEpoch(10));
    let (account, proof, sk) = test.create_funded_account();
    let template = test.get_template_address("FamiliarTap");
    let tx = test
        .transaction()
        .call_method(account, "withdraw", args![XTR, Amount::from(funds)])
        .put_last_instruction_output_on_workspace("funds")
        .call_function(template, "new", args![Workspace("funds"), Amount::from(PER_TAKE), 2u64])
        .finish()
        .seal(&sk);
    test.execute_expect_success(tx, vec![proof]);
    let tap = test
        .return_values()
        .iter()
        .find_map(|v| v.decoded::<ComponentAddress>().ok())
        .expect("tap address");
    (test, tap, account, sk)
}

fn take_tx(
    test: &TemplateTest,
    tap: ComponentAddress,
    account: ComponentAddress,
    sk: &RistrettoSecretKey,
) -> Transaction {
    test.transaction()
        .call_method(tap, "take", args![])
        .put_last_instruction_output_on_workspace("t")
        .call_method(account, "deposit", args![Workspace("t")])
        .finish()
        .seal(sk)
}

fn take_ok(test: &mut TemplateTest, tap: ComponentAddress, account: ComponentAddress, sk: &RistrettoSecretKey) {
    let tx = take_tx(test, tap, account, sk);
    test.execute_expect_success(tx, vec![]);
}

fn take_refused(
    test: &mut TemplateTest,
    tap: ComponentAddress,
    account: ComponentAddress,
    sk: &RistrettoSecretKey,
    expected: &str,
) {
    let tx = take_tx(test, tap, account, sk);
    let reason = test.execute_expect_failure(tx, vec![]);
    assert_reject_reason(&reason, expected);
}

#[test]
fn the_tap_gives_a_fixed_amount_a_few_times_per_epoch() {
    let (mut test, tap, account, sk) = tap_world(1000);
    take_ok(&mut test, tap, account, &sk);
    take_ok(&mut test, tap, account, &sk);
    take_refused(&mut test, tap, account, &sk, "tap: 2 takes this epoch already");
    test.set_virtual_substate(VirtualSubstateId::CurrentEpoch, VirtualSubstate::CurrentEpoch(11));
    take_ok(&mut test, tap, account, &sk);
    let left: Amount = test.call_method(tap, "left", args![], vec![]);
    assert_eq!(left, Amount::from(1000 - 3 * PER_TAKE));
}

#[test]
fn an_empty_tap_says_so() {
    let (mut test, tap, account, sk) = tap_world(PER_TAKE + 50);
    take_ok(&mut test, tap, account, &sk);
    take_refused(&mut test, tap, account, &sk, "tap: empty");
}
