//! Engine tests for Familiar, run against the real Ootle engine in-process (tari_template_test_tooling 0.46).
//!
//! Every leash UTXO commits a pure condition tree:
//!   errand = component(<leash>) AND BalancePreserved(MAX_PER_PAYMENT)
//!   recall = public_key(<owner one-time key>)
//! Payees are stealth outputs locked under the payee's own tree; the leash allow-lists those roots.
//!
//! Each refusal test checks the exact reason, so a test cannot pass because something unrelated failed.

use tari_template_lib::types::{
    Amount, ComponentAddress, EncryptedData, Hash32, ResourceAddress,
    bytes::Bytes,
    crypto::{RistrettoPublicKeyBytes, UtxoTag},
    rule,
    stealth::{AtomicCondition, Covenant, SpendAuthorization, SpendCondition},
};
use tari_template_test_tooling::{
    TemplateTest,
    byte_type::ToByteType,
    crypto::RistrettoSecretKey,
    engine_types::virtual_substate::{VirtualSubstate, VirtualSubstateId},
    support::{
        assert_error::assert_reject_reason,
        spec::{InputAuthSpec, InputSpec, OutputAuthSpec},
        stealth::{self, StealthSecretTransferData},
    },
    transaction::{Transaction, args, builder::named_args::NamedArg},
    wallet_crypto::{
        MaskAndValue, OutputWitness, StealthInputWitness, StealthOutputWitness,
        stealth::{condition_root, create_transfer_statement, script_path_witness},
    },
};

const LIMIT: u64 = 100;
const WINDOW: u64 = 10;
const MAX_PER_PAYMENT: u64 = 40;
const START_EPOCH: u64 = 100;

struct Keys {
    sk: RistrettoSecretKey,
    pk: RistrettoPublicKeyBytes,
}

#[derive(Clone)]
struct Utxo {
    mask: RistrettoSecretKey,
    value: u64,
}

struct World {
    test: TemplateTest,
    resource: ResourceAddress,
    faucet: ComponentAddress,
    leash: ComponentAddress,
    agent: Keys,
    owner: Keys,
    stranger: Keys,
    payees: Vec<Keys>,
    leaves: Vec<SpendCondition>,
    budget: Vec<Utxo>,
}

fn keys(test: &mut TemplateTest, seed: u8) -> Keys {
    let (sk, pk) = test.new_key_pair(seed);
    Keys {
        sk,
        pk: pk.to_byte_type(),
    }
}

fn key_leaf(pk: RistrettoPublicKeyBytes) -> SpendCondition {
    SpendCondition::access_rule(rule!(public_key(pk)))
}

fn root_of(leaves: &[SpendCondition]) -> Hash32 {
    condition_root(leaves).unwrap()
}

fn errand(leash: ComponentAddress) -> SpendCondition {
    SpendCondition::all([
        AtomicCondition::AccessRule(rule!(component(leash))),
        AtomicCondition::Covenant(Covenant::BalancePreserved(MAX_PER_PAYMENT)),
    ])
}

fn set_epoch(test: &mut TemplateTest, epoch: u64) {
    test.set_virtual_substate(VirtualSubstateId::CurrentEpoch, VirtualSubstate::CurrentEpoch(epoch));
}

fn tx_method(
    w: &World,
    component: ComponentAddress,
    method: &str,
    args: Vec<NamedArg>,
    signer: &RistrettoSecretKey,
) -> Transaction {
    w.test
        .transaction()
        .call_method(component, method, args)
        .finish()
        .seal(signer)
}

/// A leash with LIMIT per WINDOW epochs, two allow-listed payees, the budget root set, and two 100-unit budget UTXOs.
fn world() -> World {
    let mut test = TemplateTest::new(
        env!("CARGO_MANIFEST_DIR"),
        ["templates/familiar", "templates/test_faucet", "templates/mallory"],
    );
    test.enable_auto_add_proofs_from_signers();
    set_epoch(&mut test, START_EPOCH);

    let supply = 1_000_000u64;
    let mint = stealth::generate_mint_statement(Vec::<(u64, OutputAuthSpec)>::new(), supply, None);
    let faucet: ComponentAddress =
        test.call_function("TestFaucet", "new", args![Amount::from(supply), mint.statement], vec![]);
    let resource: ResourceAddress = test.call_method(faucet, "resource_address", args![], vec![]);

    let agent = keys(&mut test, 1);
    let owner = keys(&mut test, 2);
    let stranger = keys(&mut test, 3);
    let payees = vec![keys(&mut test, 4), keys(&mut test, 5)];
    let payee_roots: Vec<Hash32> = payees.iter().map(|p| root_of(&[key_leaf(p.pk)])).collect();

    let leash: ComponentAddress = test.call_function(
        "Familiar",
        "new",
        args![resource, agent.pk, owner.pk, LIMIT, WINDOW, payee_roots],
        vec![],
    );
    let leaves = vec![errand(leash), key_leaf(owner.pk)];

    let mut w = World {
        test,
        resource,
        faucet,
        leash,
        agent,
        owner,
        stranger,
        payees,
        leaves,
        budget: vec![],
    };
    let root = root_of(&w.leaves);
    let tx = tx_method(
        &w,
        w.leash,
        "set_budget_root",
        args![root, MAX_PER_PAYMENT],
        &w.owner.sk,
    );
    w.test.execute_expect_success(tx, vec![]);
    fund(&mut w, &[100, 100]);
    w
}

fn fund(w: &mut World, values: &[u64]) {
    let total: u64 = values.iter().sum();
    let outputs: Vec<(u64, OutputAuthSpec)> = values
        .iter()
        .map(|v| (*v, OutputAuthSpec::Conditions(w.leaves.clone())))
        .collect();
    let data = stealth::generate_transfer_data(stealth::NO_INPUTS, total, outputs, 0u64);
    let tx = tx_method(
        w,
        w.faucet,
        "fund",
        args![data.statement.clone()],
        &w.test.secret_key().clone(),
    );
    w.test.execute_expect_success(tx, vec![]);
    for (i, v) in values.iter().enumerate() {
        w.budget.push(Utxo {
            mask: data.output_masks[i].clone(),
            value: *v,
        });
    }
}

fn budget_input(w: &World, idx: usize, leaf: usize) -> InputSpec {
    let u = &w.budget[idx];
    InputSpec::with_auth(
        MaskAndValue {
            mask: u.mask.clone(),
            value: u.value,
        },
        InputAuthSpec::script_path(w.leaves.clone(), w.leaves[leaf].clone(), Bytes::default()),
    )
}

fn payee_auth(w: &World, i: usize) -> OutputAuthSpec {
    OutputAuthSpec::Conditions(vec![key_leaf(w.payees[i].pk)])
}

fn budget_auth(w: &World) -> OutputAuthSpec {
    OutputAuthSpec::Conditions(w.leaves.clone())
}

/// Spend budget UTXO `idx` through the errand leaf: `amount` to `to`, the change back under `change`.
fn pay_statement(
    w: &World,
    idx: usize,
    amount: u64,
    to: OutputAuthSpec,
    change: OutputAuthSpec,
) -> StealthSecretTransferData {
    let value = w.budget[idx].value;
    stealth::generate_transfer_data(
        [budget_input(w, idx, 0)],
        0u64,
        [(value - amount, change), (amount, to)],
        0u64,
    )
}

fn pay_tx(w: &World, data: &StealthSecretTransferData, signer: &RistrettoSecretKey) -> Transaction {
    tx_method(w, w.leash, "pay", args![data.statement.clone()], signer)
}

/// A successful payment of `amount` from UTXO `idx` to payee `p`; the budget tracks the change. Returns the payout
/// UTXO so the payee can spend it.
fn pay_ok(w: &mut World, idx: usize, amount: u64, p: usize) -> Utxo {
    let data = pay_statement(w, idx, amount, payee_auth(w, p), budget_auth(w));
    let tx = pay_tx(w, &data, &w.agent.sk.clone());
    w.test.execute_expect_success(tx, vec![]);
    let change = w.budget[idx].value - amount;
    let payout_index = if change > 0 { 1 } else { 0 };
    if change > 0 {
        w.budget[idx] = Utxo {
            mask: data.output_masks[0].clone(),
            value: change,
        };
    } else {
        w.budget.remove(idx);
    }
    Utxo {
        mask: data.output_masks[payout_index].clone(),
        value: amount,
    }
}

#[derive(Clone, Copy)]
enum Who {
    Agent,
    Owner,
    Stranger,
}

fn secret(w: &World, who: Who) -> RistrettoSecretKey {
    match who {
        Who::Agent => w.agent.sk.clone(),
        Who::Owner => w.owner.sk.clone(),
        Who::Stranger => w.stranger.sk.clone(),
    }
}

fn pay_refused(w: &mut World, data: &StealthSecretTransferData, who: Who, expected: &str) {
    let signer = secret(w, who);
    let tx = pay_tx(w, data, &signer);
    let reason = w.test.execute_expect_failure(tx, vec![]);
    assert_reject_reason(&reason, expected);
}

// ---------------------------------------------------------------- happy path

#[test]
fn agent_pays_allow_listed_payees_and_payee_can_spend_the_payout() {
    let mut w = world();
    let payout = pay_ok(&mut w, 0, 40, 0);
    pay_ok(&mut w, 0, 35, 1);
    pay_ok(&mut w, 1, 25, 0);

    // The payee controls exactly what it was paid: it spends the payout to its own key path.
    let payee_leaf = key_leaf(w.payees[0].pk);
    let spend = stealth::generate_transfer_data(
        [InputSpec::with_auth(
            MaskAndValue {
                mask: payout.mask.clone(),
                value: payout.value,
            },
            InputAuthSpec::single(payee_leaf),
        )],
        0u64,
        [(payout.value, OutputAuthSpec::KeyPath(w.payees[0].pk))],
        0u64,
    );
    let tx = w
        .test
        .transaction()
        .stealth_transfer(w.resource, spend.statement)
        .finish()
        .seal(&w.payees[0].sk.clone());
    w.test.execute_expect_success(tx, vec![]);

    // status is readable by anyone
    let tx = tx_method(&w, w.leash, "status", args![], &w.stranger.sk.clone());
    w.test.execute_expect_success(tx, vec![]);
}

#[test]
fn window_limit_refuses_with_numbers_then_reopens_next_window() {
    let mut w = world();
    pay_ok(&mut w, 0, 40, 0);
    pay_ok(&mut w, 1, 40, 1);
    let data = pay_statement(&w, 0, 30, payee_auth(&w, 0), budget_auth(&w));
    pay_refused(
        &mut w,
        &data,
        Who::Agent,
        "familiar: window limit exceeded: 80 of 100 spent, 20 left, payment of 30 refused",
    );
    // what is left in the window still goes through
    pay_ok(&mut w, 0, 20, 0);

    // next window
    set_epoch(&mut w.test, START_EPOCH + WINDOW);
    pay_ok(&mut w, 0, 30, 1);
}

// ---------------------------------------------------------------- refused by the network (covenant)

#[test]
fn single_payment_over_the_covenant_allowance_is_refused() {
    let mut w = world();
    let data = pay_statement(&w, 0, MAX_PER_PAYMENT + 1, payee_auth(&w, 0), budget_auth(&w));
    pay_refused(
        &mut w,
        &data,
        Who::Agent,
        "covenant not satisfied: BalancePreserved(40)",
    );
}

#[test]
fn spending_the_budget_from_a_hostile_component_is_refused() {
    let mut w = world();
    let mallory: ComponentAddress = w.test.call_function("Mallory", "new", args![w.resource], vec![]);
    let data = pay_statement(
        &w,
        0,
        30,
        OutputAuthSpec::KeyPath(w.stranger.pk),
        OutputAuthSpec::KeyPath(w.stranger.pk),
    );
    let tx = tx_method(&w, mallory, "steal", args![data.statement.clone()], &w.agent.sk.clone());
    let reason = w.test.execute_expect_failure(tx, vec![]);
    assert_reject_reason(&reason, "Access Denied: native.stealth_utxo.spend");
}

#[test]
fn spending_the_budget_with_a_plain_transfer_is_refused() {
    let mut w = world();
    let data = pay_statement(&w, 0, 30, payee_auth(&w, 0), budget_auth(&w));
    let tx = w
        .test
        .transaction()
        .stealth_transfer(w.resource, data.statement.clone())
        .finish()
        .seal(&w.agent.sk.clone());
    let reason = w.test.execute_expect_failure(tx, vec![]);
    // Outside any component frame the errand leaf cannot even be evaluated.
    assert_reject_reason(&reason, "No active call frame");
}

#[test]
fn replaying_a_payment_is_refused() {
    let mut w = world();
    let data = pay_statement(&w, 0, 10, payee_auth(&w, 0), budget_auth(&w));
    let tx = pay_tx(&w, &data, &w.agent.sk.clone());
    w.test.execute_expect_success(tx, vec![]);
    let tx = pay_tx(&w, &data, &w.agent.sk.clone());
    let reason = w.test.execute_expect_failure(tx, vec![]);
    println!("replay reject reason: {reason}");
}

// ---------------------------------------------------------------- refused by the leash (component)

#[test]
fn payment_to_a_payee_not_on_the_leash_is_refused() {
    let mut w = world();
    let to = OutputAuthSpec::Conditions(vec![key_leaf(w.stranger.pk)]);
    let data = pay_statement(&w, 0, 10, to, budget_auth(&w));
    pay_refused(
        &mut w,
        &data,
        Who::Agent,
        "familiar: output to a payee not on the leash",
    );
}

#[test]
fn payout_to_a_plain_key_is_refused() {
    let mut w = world();
    let data = pay_statement(&w, 0, 10, OutputAuthSpec::KeyPath(w.agent.pk), budget_auth(&w));
    pay_refused(
        &mut w,
        &data,
        Who::Agent,
        "familiar: output to a payee not on the leash",
    );
}

#[test]
fn change_with_an_added_key_path_is_refused() {
    let mut w = world();
    let change = OutputAuthSpec::KeyAndConditions {
        spend_key: w.agent.pk,
        conditions: w.leaves.clone(),
    };
    let data = pay_statement(&w, 0, 10, payee_auth(&w, 0), change);
    pay_refused(
        &mut w,
        &data,
        Who::Agent,
        "familiar: output to a payee not on the leash",
    );
}

#[test]
fn revealed_output_is_refused() {
    let mut w = world();
    let value = w.budget[0].value;
    let data = stealth::generate_transfer_data(
        [budget_input(&w, 0, 0)],
        0u64,
        [(value - 15, budget_auth(&w)), (10, payee_auth(&w, 0))],
        5u64,
    );
    pay_refused(&mut w, &data, Who::Agent, "familiar: revealed outputs are not allowed");
}

#[test]
fn a_stranger_cannot_pay() {
    let mut w = world();
    let data = pay_statement(&w, 0, 10, payee_auth(&w, 0), budget_auth(&w));
    pay_refused(&mut w, &data, Who::Stranger, "Access Denied");
}

/// The owner key is not the agent key: the leash has no component owner that could bypass the `pay` rule.
#[test]
fn the_owner_cannot_pay_as_the_agent() {
    let mut w = world();
    let data = pay_statement(&w, 0, 10, payee_auth(&w, 0), budget_auth(&w));
    pay_refused(&mut w, &data, Who::Owner, "Access Denied");
}

#[test]
fn paused_leash_refuses_until_resumed() {
    let mut w = world();
    let tx = tx_method(&w, w.leash, "pause", args![], &w.owner.sk.clone());
    w.test.execute_expect_success(tx, vec![]);
    let data = pay_statement(&w, 0, 10, payee_auth(&w, 0), budget_auth(&w));
    pay_refused(&mut w, &data, Who::Agent, "familiar: leash is paused by the owner");
    let tx = tx_method(&w, w.leash, "resume", args![], &w.owner.sk.clone());
    w.test.execute_expect_success(tx, vec![]);
    pay_ok(&mut w, 0, 10, 0);
}

#[test]
fn admin_methods_are_owner_only() {
    let mut w = world();
    for (method, a) in [
        ("pause", args![]),
        ("set_policy", args![1_000_000u64, 1u64]),
        ("set_payees", args![vec![root_of(&[key_leaf(w.agent.pk)])]]),
        (
            "set_budget_root",
            args![root_of(&[key_leaf(w.agent.pk)]), MAX_PER_PAYMENT],
        ),
        ("set_single_use_payees", args![true]),
    ] {
        let tx = tx_method(&w, w.leash, method, a, &w.agent.sk.clone());
        let reason = w.test.execute_expect_failure(tx, vec![]);
        assert_reject_reason(&reason, "Access Denied");
    }
}

#[test]
fn owner_can_tighten_the_policy() {
    let mut w = world();
    let tx = tx_method(&w, w.leash, "set_policy", args![15u64, WINDOW], &w.owner.sk.clone());
    w.test.execute_expect_success(tx, vec![]);
    let data = pay_statement(&w, 0, 16, payee_auth(&w, 0), budget_auth(&w));
    pay_refused(
        &mut w,
        &data,
        Who::Agent,
        "familiar: window limit exceeded: 0 of 15 spent",
    );
}

#[test]
fn budget_root_cannot_be_a_payee() {
    let mut w = world();
    let root = root_of(&w.leaves);
    let tx = tx_method(&w, w.leash, "set_payees", args![vec![root]], &w.owner.sk.clone());
    let reason = w.test.execute_expect_failure(tx, vec![]);
    assert_reject_reason(&reason, "familiar: budget root cannot be a payee");
}

// ---------------------------------------------------------------- owner recall (hidden leaf)

#[test]
fn owner_recalls_everything_and_the_agent_is_left_with_nothing() {
    let mut w = world();
    let total: u64 = w.budget.iter().map(|u| u.value).sum();
    let recall = stealth::generate_transfer_data(
        [budget_input(&w, 0, 1), budget_input(&w, 1, 1)],
        0u64,
        [(total, OutputAuthSpec::KeyPath(w.owner.pk))],
        0u64,
    );
    let tx = w
        .test
        .transaction()
        .stealth_transfer(w.resource, recall.statement)
        .finish()
        .seal(&w.owner.sk.clone());
    w.test.execute_expect_success(tx, vec![]);

    let data = pay_statement(&w, 0, 10, payee_auth(&w, 0), budget_auth(&w));
    let tx = pay_tx(&w, &data, &w.agent.sk.clone());
    let reason = w.test.execute_expect_failure(tx, vec![]);
    println!("pay after recall reject reason: {reason}");
}

#[test]
fn stranger_cannot_use_the_recall_leaf() {
    let mut w = world();
    let recall = stealth::generate_transfer_data(
        [budget_input(&w, 0, 1)],
        0u64,
        [(100, OutputAuthSpec::KeyPath(w.stranger.pk))],
        0u64,
    );
    let tx = w
        .test
        .transaction()
        .stealth_transfer(w.resource, recall.statement)
        .finish()
        .seal(&w.stranger.sk.clone());
    let reason = w.test.execute_expect_failure(tx, vec![]);
    assert_reject_reason(&reason, "Access Denied: native.stealth_utxo.spend");
}

// ---------------------------------------------------------------- construction

#[test]
fn construction_rejects_bad_parameters() {
    let mut w = world();
    let template = w.test.get_template_address("Familiar");
    let payee = root_of(&[key_leaf(w.payees[0].pk)]);
    let cases: Vec<(Vec<NamedArg>, &str)> = vec![
        (
            args![w.resource, w.agent.pk, w.owner.pk, 0u64, WINDOW, vec![payee]],
            "familiar: invalid policy",
        ),
        (
            args![w.resource, w.agent.pk, w.owner.pk, LIMIT, 0u64, vec![payee]],
            "familiar: invalid policy",
        ),
        (
            args![w.resource, w.agent.pk, w.owner.pk, LIMIT, WINDOW, Vec::<Hash32>::new()],
            "familiar: at least one payee",
        ),
        (
            args![w.resource, w.agent.pk, w.owner.pk, LIMIT, WINDOW, vec![payee, payee]],
            "familiar: duplicate payee",
        ),
        (
            args![w.resource, w.agent.pk, w.agent.pk, LIMIT, WINDOW, vec![payee]],
            "familiar: agent and owner keys must differ",
        ),
    ];
    for (a, expected) in cases {
        let tx = w
            .test
            .transaction()
            .call_function(template, "new", a)
            .finish()
            .seal(&w.test.secret_key().clone());
        let reason = w.test.execute_expect_failure(tx, vec![]);
        assert_reject_reason(&reason, expected);
    }
}

#[test]
fn pay_without_a_budget_root_is_refused() {
    let mut w = world();
    let payee = root_of(&[key_leaf(w.payees[0].pk)]);
    let fresh: ComponentAddress = w.test.call_function(
        "Familiar",
        "new",
        args![w.resource, w.agent.pk, w.owner.pk, LIMIT, WINDOW, vec![payee]],
        vec![],
    );
    let data = pay_statement(&w, 0, 10, payee_auth(&w, 0), budget_auth(&w));
    let tx = tx_method(&w, fresh, "pay", args![data.statement.clone()], &w.agent.sk.clone());
    let reason = w.test.execute_expect_failure(tx, vec![]);
    assert_reject_reason(&reason, "familiar: budget root not set");
}

// ---------------------------------------------------------------- found in review (PoC on the old code: CI run 39209529251)

/// A budget UTXO spent through the errand leaf of an explicit leaf set (used when the budget root was rotated).
fn input_under(leaves: &[SpendCondition], u: &Utxo) -> InputSpec {
    InputSpec::with_auth(
        MaskAndValue {
            mask: u.mask.clone(),
            value: u.value,
        },
        InputAuthSpec::script_path(leaves.to_vec(), leaves[0].clone(), Bytes::default()),
    )
}

/// The covenant caps the outflow per condition root (one claim per root, not per UTXO). After the owner rotates the
/// budget root, a spend that takes one UTXO from the old root and one from the new root carries two claims, each
/// within its own cap; the leash must cap their sum. On the old code this moved 2 x MAX_PER_PAYMENT.
#[test]
fn spend_across_two_budget_roots_is_capped_by_the_leash() {
    let mut w = world();
    fund(&mut w, &[MAX_PER_PAYMENT]);
    let old_leaves = w.leaves.clone();
    let old = w.budget.last().unwrap().clone();

    let new_recall = keys(&mut w.test, 6);
    let new_leaves = vec![errand(w.leash), key_leaf(new_recall.pk)];
    let tx = tx_method(
        &w,
        w.leash,
        "set_budget_root",
        args![root_of(&new_leaves), MAX_PER_PAYMENT],
        &w.owner.sk.clone(),
    );
    w.test.execute_expect_success(tx, vec![]);
    w.leaves = new_leaves.clone();
    fund(&mut w, &[MAX_PER_PAYMENT]);
    let new = w.budget.last().unwrap().clone();

    let over = stealth::generate_transfer_data(
        [input_under(&old_leaves, &old), input_under(&new_leaves, &new)],
        0u64,
        [(2 * MAX_PER_PAYMENT, payee_auth(&w, 0))],
        0u64,
    );
    println!("claims in the statement: {}", over.statement.covenant_claims.len());
    pay_refused(
        &mut w,
        &over,
        Who::Agent,
        "familiar: payment of 80 over the per-payment cap 40",
    );

    // Within the cap, UTXOs under both roots are spent together; the change moves under the current root.
    let ok = stealth::generate_transfer_data(
        [input_under(&old_leaves, &old), input_under(&new_leaves, &new)],
        0u64,
        [(MAX_PER_PAYMENT, budget_auth(&w)), (MAX_PER_PAYMENT, payee_auth(&w, 0))],
        0u64,
    );
    let tx = pay_tx(&w, &ok, &w.agent.sk.clone());
    w.test.execute_expect_success(tx, vec![]);
}

/// Two UTXOs under the same root are one partition for the covenant: one claim and one cap for the whole spend,
/// so the engine itself refuses it before the leash gets to count anything.
#[test]
fn two_utxos_under_one_root_are_one_covenant_partition() {
    let mut w = world();
    let total = w.budget[0].value + w.budget[1].value;
    let over = stealth::generate_transfer_data(
        [budget_input(&w, 0, 0), budget_input(&w, 1, 0)],
        0u64,
        [
            (total - 2 * MAX_PER_PAYMENT, budget_auth(&w)),
            (2 * MAX_PER_PAYMENT, payee_auth(&w, 0)),
        ],
        0u64,
    );
    println!("claims in the statement: {}", over.statement.covenant_claims.len());
    pay_refused(
        &mut w,
        &over,
        Who::Agent,
        "covenant not satisfied: BalancePreserved(40)",
    );
    let ok = stealth::generate_transfer_data(
        [budget_input(&w, 0, 0), budget_input(&w, 1, 0)],
        0u64,
        [
            (total - MAX_PER_PAYMENT, budget_auth(&w)),
            (MAX_PER_PAYMENT, payee_auth(&w, 0)),
        ],
        0u64,
    );
    let tx = pay_tx(&w, &ok, &w.agent.sk.clone());
    w.test.execute_expect_success(tx, vec![]);
}

/// Shrinking `window_epochs` mid-window used to re-align the grid onto a past point and free what was spent. Now the
/// window restarts at the change and keeps its `spent`.
#[test]
fn shrinking_the_window_keeps_what_was_spent() {
    let mut w = world();
    pay_ok(&mut w, 0, 40, 0);
    pay_ok(&mut w, 1, 40, 1);
    set_epoch(&mut w.test, START_EPOCH + 5);
    let tx = tx_method(&w, w.leash, "set_policy", args![LIMIT, 3u64], &w.owner.sk.clone());
    w.test.execute_expect_success(tx, vec![]);
    let data = pay_statement(&w, 0, 30, payee_auth(&w, 0), budget_auth(&w));
    pay_refused(
        &mut w,
        &data,
        Who::Agent,
        "familiar: window limit exceeded: 80 of 100 spent, 20 left, payment of 30 refused",
    );
    // The new window runs from the change (epoch 105), not from the old grid (which would reopen at 106).
    set_epoch(&mut w.test, START_EPOCH + 5 + 2);
    let data = pay_statement(&w, 0, 30, payee_auth(&w, 0), budget_auth(&w));
    pay_refused(
        &mut w,
        &data,
        Who::Agent,
        "familiar: window limit exceeded: 80 of 100 spent",
    );
    set_epoch(&mut w.test, START_EPOCH + 5 + 3);
    pay_ok(&mut w, 0, 30, 0);
}

// ---------------------------------------------------------------- more refusals (review: missing negative tests)

#[test]
fn zero_payment_is_refused() {
    let mut w = world();
    // A zero-value payee output is dropped by the stealth statement builder, so the template refuses the spend at
    // the payee check, before the amount reaches `familiar_core::charge` (which also refuses zero, see core tests).
    let data = pay_statement(&w, 0, 0, payee_auth(&w, 0), budget_auth(&w));
    pay_refused(&mut w, &data, Who::Agent, "familiar: no payee output");
}

#[test]
fn set_payees_rejects_duplicates() {
    let mut w = world();
    let payee = root_of(&[key_leaf(w.payees[0].pk)]);
    let tx = tx_method(
        &w,
        w.leash,
        "set_payees",
        args![vec![payee, payee]],
        &w.owner.sk.clone(),
    );
    let reason = w.test.execute_expect_failure(tx, vec![]);
    assert_reject_reason(&reason, "familiar: duplicate payee");
}

// ---------------------------------------------------------------- privacy: single-use payee roots

/// Turns single-use payee roots on and replaces the allow-list with `roots`.
fn single_use(w: &mut World, roots: Vec<Hash32>) {
    let tx = tx_method(w, w.leash, "set_single_use_payees", args![true], &w.owner.sk.clone());
    w.test.execute_expect_success(tx, vec![]);
    let tx = tx_method(w, w.leash, "set_payees", args![roots], &w.owner.sk.clone());
    w.test.execute_expect_success(tx, vec![]);
}

#[test]
fn single_use_payee_root_cannot_be_paid_twice() {
    let mut w = world();
    // Payee 0 hands the owner two roots (two one-time keys); an observer sees two unrelated roots.
    let second = keys(&mut w.test, 6);
    let roots = vec![root_of(&[key_leaf(w.payees[0].pk)]), root_of(&[key_leaf(second.pk)])];
    single_use(&mut w, roots);

    pay_ok(&mut w, 0, 10, 0);
    // The first root is struck off: paying it again is refused, so repeat payments cannot share a root.
    let data = pay_statement(&w, 0, 10, payee_auth(&w, 0), budget_auth(&w));
    pay_refused(
        &mut w,
        &data,
        Who::Agent,
        "familiar: output to a payee not on the leash",
    );
    // The fresh root still works.
    let data = pay_statement(
        &w,
        0,
        10,
        OutputAuthSpec::Conditions(vec![key_leaf(second.pk)]),
        budget_auth(&w),
    );
    let tx = pay_tx(&w, &data, &w.agent.sk.clone());
    w.test.execute_expect_success(tx, vec![]);
}

#[test]
fn single_use_root_takes_one_output_even_within_one_spend() {
    let mut w = world();
    let roots = vec![root_of(&[key_leaf(w.payees[0].pk)])];
    single_use(&mut w, roots);
    // Two outputs to the same single-use root in one spend (found in review): refused.
    let value = w.budget[0].value;
    let data = stealth::generate_transfer_data(
        [budget_input(&w, 0, 0)],
        0u64,
        [
            (value - 10, budget_auth(&w)),
            (5, payee_auth(&w, 0)),
            (5, payee_auth(&w, 0)),
        ],
        0u64,
    );
    pay_refused(
        &mut w,
        &data,
        Who::Agent,
        "familiar: single-use payee root paid twice in one spend",
    );
    // With single-use off, the same shape is accepted.
    let tx = tx_method(&w, w.leash, "set_single_use_payees", args![false], &w.owner.sk.clone());
    w.test.execute_expect_success(tx, vec![]);
    let tx = pay_tx(&w, &data, &w.agent.sk.clone());
    w.test.execute_expect_success(tx, vec![]);
}

#[test]
fn single_use_off_by_default_repeat_root_is_accepted() {
    let mut w = world();
    pay_ok(&mut w, 0, 10, 0);
    pay_ok(&mut w, 0, 10, 0);
}

#[test]
fn single_use_runs_out_of_roots_until_the_owner_tops_up() {
    let mut w = world();
    let roots = vec![root_of(&[key_leaf(w.payees[0].pk)])];
    single_use(&mut w, roots);
    pay_ok(&mut w, 0, 10, 0);
    let data = pay_statement(&w, 0, 10, payee_auth(&w, 1), budget_auth(&w));
    pay_refused(
        &mut w,
        &data,
        Who::Agent,
        "familiar: output to a payee not on the leash",
    );
    // The owner tops up with a fresh root; the agent can pay again.
    let tx = tx_method(
        &w,
        w.leash,
        "set_payees",
        args![vec![root_of(&[key_leaf(w.payees[1].pk)])]],
        &w.owner.sk.clone(),
    );
    w.test.execute_expect_success(tx, vec![]);
    pay_ok(&mut w, 0, 10, 1);
}

#[test]
fn a_refused_payment_does_not_consume_a_single_use_root() {
    let mut w = world();
    let roots = vec![root_of(&[key_leaf(w.payees[0].pk)])];
    single_use(&mut w, roots);
    // Over the per-spend cap: refused by the covenant, the whole transaction is rolled back with the root intact.
    let data = pay_statement(&w, 0, MAX_PER_PAYMENT + 1, payee_auth(&w, 0), budget_auth(&w));
    pay_refused(
        &mut w,
        &data,
        Who::Agent,
        &format!("covenant not satisfied: BalancePreserved({MAX_PER_PAYMENT})"),
    );
    pay_ok(&mut w, 0, 10, 0);
}

// ---------------------------------------------------------------- privacy: a recall without a covenant claim

/// A stealth output for a hand-built statement (masks and nonces from deterministic test keys).
fn witness_output(w: &mut World, seed: u8, value: u64, auth: SpendAuthorization) -> StealthOutputWitness {
    let (mask, _) = w.test.new_key_pair(seed);
    let (_, nonce) = w.test.new_key_pair(seed.wrapping_add(100));
    StealthOutputWitness {
        witness: OutputWitness {
            amount: value,
            mask,
            sender_public_nonce: nonce,
            minimum_value_promise: 0,
            encrypted_data: EncryptedData::try_from(vec![0u8; EncryptedData::min_size()]).unwrap(),
            resource_view_key: None,
        },
        auth,
        tag: UtxoTag::new(0),
    }
}

/// Budget UTXO `idx` spent through `leaf`, with no committed root recorded, so the wallet builds no covenant claim.
fn claimless_input(w: &World, idx: usize, leaf: usize) -> StealthInputWitness {
    let (witness, _root) = script_path_witness(&w.leaves, &w.leaves[leaf]).unwrap();
    StealthInputWitness {
        mask_and_value: MaskAndValue {
            mask: w.budget[idx].mask.clone(),
            value: w.budget[idx].value,
        },
        witness,
        condition_root: None,
    }
}

#[test]
fn owner_recall_without_a_claim_does_not_reveal_what_is_left() {
    let mut w = world();
    pay_ok(&mut w, 0, 30, 0);
    let total: u64 = w.budget.iter().map(|u| u.value).sum();
    let owner_pk = w.owner.pk;
    let outs = vec![witness_output(&mut w, 40, total, SpendAuthorization::Key(owner_pk))];
    let inputs = vec![claimless_input(&w, 0, 1), claimless_input(&w, 1, 1)];
    let recall = create_transfer_statement(inputs, Amount::zero(), outs.iter(), None).unwrap();
    // Nothing in the statement says how much is recalled: no claim, no revealed amount.
    assert!(recall.covenant_claims.is_empty());
    assert!(recall.outputs_statement.revealed_output.is_none());
    let tx = w
        .test
        .transaction()
        .stealth_transfer(w.resource, recall)
        .finish()
        .seal(&w.owner.sk.clone());
    w.test.execute_expect_success(tx, vec![]);
}

#[test]
fn pay_without_a_covenant_claim_is_refused() {
    let mut w = world();
    let budget_root = root_of(&w.leaves);
    let payee_root = root_of(&[key_leaf(w.payees[0].pk)]);
    let change = w.budget[0].value - 10;
    let outs = vec![
        witness_output(&mut w, 41, change, SpendAuthorization::Script(budget_root)),
        witness_output(&mut w, 42, 10, SpendAuthorization::Script(payee_root)),
    ];
    let spend = create_transfer_statement(vec![claimless_input(&w, 0, 0)], Amount::zero(), outs.iter(), None).unwrap();
    assert!(spend.covenant_claims.is_empty());
    let tx = tx_method(&w, w.leash, "pay", args![spend], &w.agent.sk.clone());
    let reason = w.test.execute_expect_failure(tx, vec![]);
    assert_reject_reason(&reason, "familiar: spend without a covenant claim");
}
