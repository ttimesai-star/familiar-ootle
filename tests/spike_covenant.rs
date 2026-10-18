//! S1 spike: can a stealth UTXO be locked so that it is spendable ONLY from inside the Familiar component, with a
//! native per-spend allowance, while the owner keeps a hidden recall leaf?
//!
//! Tree committed by every leash UTXO (pure `Script(root)`, no key path):
//!   errand = component(<leash>) AND BalancePreserved(ALLOWANCE)
//!   recall = public_key(<owner one-time key>)

use tari_template_lib::types::{
    Amount,
    ComponentAddress,
    ResourceAddress,
    bytes::Bytes,
    crypto::RistrettoPublicKeyBytes,
    rule,
    stealth::{AtomicCondition, Covenant, SpendCondition},
};
use tari_template_test_tooling::{
    TemplateTest,
    byte_type::ToByteType,
    crypto::RistrettoSecretKey,
    support::{
        spec::{InputAuthSpec, InputSpec, OutputAuthSpec},
        stealth::{self, StealthSecretTransferData},
    },
    transaction::{Epoch, Transaction, args},
    wallet_crypto::MaskAndValue,
};

const ALLOWANCE: u64 = 30;
const UTXO_VALUE: u64 = 100;

struct World {
    test: TemplateTest,
    resource: ResourceAddress,
    faucet: ComponentAddress,
    leash: ComponentAddress,
    owner_sk: RistrettoSecretKey,
    owner_pk: RistrettoPublicKeyBytes,
    payee_pk: RistrettoPublicKeyBytes,
}

fn setup() -> World {
    let mut test = TemplateTest::new(env!("CARGO_MANIFEST_DIR"), ["templates/familiar", "templates/test_faucet"]);
    test.enable_auto_add_proofs_from_signers();

    // Faucet: the whole supply comes back as a revealed output to the default signer and stays in the faucet.
    let supply = 1_000_000u64;
    let mint = stealth::generate_mint_statement(Vec::<(u64, OutputAuthSpec)>::new(), supply, None);
    let faucet: ComponentAddress =
        test.call_function("TestFaucet", "new", args![Amount::from(supply), mint.statement], vec![]);
    let resource: ResourceAddress = test.call_method(faucet, "resource_address", args![], vec![]);
    let leash: ComponentAddress = test.call_function("Familiar", "new", args![resource], vec![]);

    let (owner_sk, owner) = test.new_key_pair(7);
    let (_payee_sk, payee) = test.new_key_pair(9);
    World {
        test,
        resource,
        faucet,
        leash,
        owner_sk,
        owner_pk: owner.to_byte_type(),
        payee_pk: payee.to_byte_type(),
    }
}

fn errand(leash: ComponentAddress, allowance: u64) -> SpendCondition {
    SpendCondition::all([
        AtomicCondition::AccessRule(rule!(component(leash))),
        AtomicCondition::Covenant(Covenant::BalancePreserved(allowance)),
    ])
}

fn errand_caller(leash: ComponentAddress, allowance: u64) -> SpendCondition {
    SpendCondition::all([
        AtomicCondition::AccessRule(rule!(caller_component(leash))),
        AtomicCondition::Covenant(Covenant::BalancePreserved(allowance)),
    ])
}

fn recall(owner: RistrettoPublicKeyBytes) -> SpendCondition {
    SpendCondition::access_rule(rule!(public_key(owner)))
}

/// Puts one UTXO of `UTXO_VALUE` under `leaves` and returns the funding secrets (output mask 0 spends it).
fn fund(w: &mut World, leaves: &[SpendCondition]) -> StealthSecretTransferData {
    let data = stealth::generate_transfer_data(
        stealth::NO_INPUTS,
        UTXO_VALUE,
        [(UTXO_VALUE, OutputAuthSpec::Conditions(leaves.to_vec()))],
        0u64,
    );
    let tx = Transaction::builder_localnet(Epoch(1))
        .call_method(w.faucet, "fund", args![data.statement.clone()])
        .finish()
        .seal(w.test.secret_key());
    w.test.execute_expect_success(tx, vec![]);
    data
}

fn input(funding: &StealthSecretTransferData, leaves: &[SpendCondition], leaf: &SpendCondition) -> InputSpec {
    InputSpec::with_auth(
        MaskAndValue {
            mask: funding.output_masks[0].clone(),
            value: UTXO_VALUE,
        },
        InputAuthSpec::script_path(leaves.to_vec(), leaf.clone(), Bytes::default()),
    )
}

/// Spend through the errand leaf: keep `UTXO_VALUE - pay` under the same tree, send `pay` to the payee key.
fn errand_spend(
    funding: &StealthSecretTransferData,
    leaves: &[SpendCondition],
    leaf: &SpendCondition,
    pay: u64,
    payee: RistrettoPublicKeyBytes,
) -> StealthSecretTransferData {
    stealth::generate_transfer_data(
        [input(funding, leaves, leaf)],
        0u64,
        [
            (UTXO_VALUE - pay, OutputAuthSpec::Conditions(leaves.to_vec())),
            (pay, OutputAuthSpec::KeyPath(payee)),
        ],
        0u64,
    )
}

fn via_component(w: &World, spend: &StealthSecretTransferData) -> Transaction {
    Transaction::builder_localnet(Epoch(1))
        .call_method(w.leash, "spend_raw", args![spend.statement.clone()])
        .finish()
        .seal(w.test.secret_key())
}

fn native(w: &World, spend: &StealthSecretTransferData, signer: &RistrettoSecretKey) -> Transaction {
    Transaction::builder_localnet(Epoch(1))
        .stealth_transfer(w.resource, spend.statement.clone())
        .finish()
        .seal(signer)
}

#[test]
fn spike_errand_inside_component_within_allowance_is_accepted() {
    let mut w = setup();
    let leaves = vec![errand(w.leash, ALLOWANCE), recall(w.owner_pk)];
    let funding = fund(&mut w, &leaves);
    let spend = errand_spend(&funding, &leaves, &leaves[0], ALLOWANCE, w.payee_pk);
    let tx = via_component(&w, &spend);
    w.test.execute_expect_success(tx, vec![]);
}

#[test]
fn spike_errand_outside_component_is_rejected() {
    let mut w = setup();
    let leaves = vec![errand(w.leash, ALLOWANCE), recall(w.owner_pk)];
    let funding = fund(&mut w, &leaves);
    let spend = errand_spend(&funding, &leaves, &leaves[0], ALLOWANCE, w.payee_pk);
    let tx = native(&w, &spend, &w.test.secret_key().clone());
    let reason = w.test.execute_expect_failure(tx, vec![]);
    println!("SPIKE outside-component reject reason: {reason}");
}

#[test]
fn spike_errand_over_allowance_is_rejected() {
    let mut w = setup();
    let leaves = vec![errand(w.leash, ALLOWANCE), recall(w.owner_pk)];
    let funding = fund(&mut w, &leaves);
    let spend = errand_spend(&funding, &leaves, &leaves[0], ALLOWANCE + 1, w.payee_pk);
    let tx = via_component(&w, &spend);
    let reason = w.test.execute_expect_failure(tx, vec![]);
    println!("SPIKE over-allowance reject reason: {reason}");
}

#[test]
fn spike_recall_by_owner_is_accepted() {
    let mut w = setup();
    let leaves = vec![errand(w.leash, ALLOWANCE), recall(w.owner_pk)];
    let funding = fund(&mut w, &leaves);
    let spend = stealth::generate_transfer_data(
        [input(&funding, &leaves, &leaves[1])],
        0u64,
        [(UTXO_VALUE, OutputAuthSpec::KeyPath(w.owner_pk))],
        0u64,
    );
    let owner_sk = w.owner_sk.clone();
    let tx = native(&w, &spend, &owner_sk);
    w.test.execute_expect_success(tx, vec![]);
}

#[test]
fn spike_recall_by_stranger_is_rejected() {
    let mut w = setup();
    let leaves = vec![errand(w.leash, ALLOWANCE), recall(w.owner_pk)];
    let funding = fund(&mut w, &leaves);
    let spend = stealth::generate_transfer_data(
        [input(&funding, &leaves, &leaves[1])],
        0u64,
        [(UTXO_VALUE, OutputAuthSpec::KeyPath(w.payee_pk))],
        0u64,
    );
    let tx = native(&w, &spend, &w.test.secret_key().clone());
    let reason = w.test.execute_expect_failure(tx, vec![]);
    println!("SPIKE stranger-recall reject reason: {reason}");
}

/// Informational: the same errand leaf gated by `caller_component` instead of `component`. Kept to learn which of the
/// two scopes the engine applies to a spend executed from inside a component frame.
#[test]
fn spike_caller_component_variant_inside_component() {
    let mut w = setup();
    let leaves = vec![errand_caller(w.leash, ALLOWANCE), recall(w.owner_pk)];
    let funding = fund(&mut w, &leaves);
    let spend = errand_spend(&funding, &leaves, &leaves[0], ALLOWANCE, w.payee_pk);
    let tx = via_component(&w, &spend);
    let result = format!("{:?}", w.test.try_execute(tx, vec![]));
    let shown: String = result.chars().take(1500).collect();
    println!("SPIKE caller_component variant: {shown}");
}
