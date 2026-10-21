//! Try Familiar yourself on Esmeralda: your own leash in coupon mode, in a few commands.
//!
//! ```text
//! familiar wallet  me.json                  # a fresh testnet key, written to me.json (never printed)
//! familiar tap     me.json                  # a few tTARI from the Familiar tap into your account
//! familiar leash   me.json                  # one-time owner and agent keys, a leash, 10 hidden coupons
//! familiar pay     me.json candle-shop 2    # the agent pays two coupons to an allow-listed payee
//! familiar ask     me.json "<any text>"     # the obedient agent reads your text and pays what it says
//! familiar pause | resume | recall | show me.json
//! ```
//!
//! The state file holds testnet secret keys and the coupons' masks: that is the whole wallet. Keep it out of any
//! repository. Every transaction is printed with an explorer link and the network's decision.

use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use ootle_rs::{
    Network, ToAccountAddress, TransactionRequest, default_indexer_url,
    key_provider::PrivateKeyProvider,
    keys::OotleSecretKey,
    provider::{Provider, ProviderBuilder, WalletProvider, WantInput},
    wallet::OotleWallet,
};
use serde::{Deserialize, Serialize};
use tari_crypto::{
    keys::PublicKey as _,
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
    tari_utilities::hex::{Hex, to_hex},
};
use tari_ootle_transaction::{Epoch, Transaction, args};
use tari_ootle_wallet_crypto::{StealthInputWitness, stealth::script_path_witness};
use tari_template_lib_types::{
    Amount, ComponentAddress, Hash32, TemplateAddress, UtxoAddress,
    constants::{TARI, TARI_TOKEN},
    crypto::PedersenCommitmentBytes,
    rule,
    stealth::{RevealedOutput, SpendAuthorization, SpendCondition, StealthTransferStatement},
};

use crate::{
    Actor, Coin, EXPLORER, FEE, Keys, Prov, Sent, agent, build_tx, coins, key_leaf, new_keys, output, recall_input,
    root, send, send_from, statement,
};

/// The Familiar template (with coupon mode) and the tTARI tap on Esmeralda, published by the author. Override with
/// FAMILIAR_TEMPLATE / FAMILIAR_TAP (for example after publishing your own copy).
pub const DEFAULT_TEMPLATE: &str = "3f4aa7a6dfc4a5541db86ff785f98404dd24c763fdc4746d423d30ea1df31d8b";
pub const DEFAULT_TAP: &str = "component_9c90f61b7665a6cff488d9f95f6a976c45139e49bda5a866fa9e6cb1b92849c7";

/// One coupon: 0.1 tTARI. Only the owner and the agent know this number; it is never written to the chain.
pub const COUPON_UT: u64 = 100_000;
const COUPONS: usize = 10;
const PER_WINDOW: u64 = 6;
const PER_PAYMENT: u64 = 4;
const WINDOW_EPOCHS: u64 = 10_000;
const PAYEES: [&str; 2] = ["candle-shop", "herb-shop"];
/// Fees for the one-time keys: the whole leash must fit in one take from the tap (5 tTARI, minus its own fee).
const OWNER_FEES: u64 = 10 * FEE;
const AGENT_FEES: u64 = 20 * FEE;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CoinJ {
    mask: String,
    value: u64,
    commitment: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ActorJ {
    sk: String,
    fee_coin: CoinJ,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PayeeJ {
    name: String,
    root: String,
}

#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Eq)]
pub struct State {
    /// The testnet account: "<account secret hex>:<view secret hex>".
    account: String,
    template: Option<String>,
    leash: Option<String>,
    owner: Option<ActorJ>,
    agent: Option<ActorJ>,
    coupon_ut: u64,
    coupons: Vec<CoinJ>,
    payees: Vec<PayeeJ>,
    /// Every transaction this wallet sent: step, tx id, accepted, reason.
    history: Vec<serde_json::Value>,
}

fn coin_to_j(c: &Coin) -> CoinJ {
    CoinJ {
        mask: c.mask.to_hex(),
        value: c.value,
        commitment: to_hex(c.commitment.as_ref()),
    }
}

fn coin_from_j(c: &CoinJ) -> Result<Coin> {
    Ok(Coin {
        mask: RistrettoSecretKey::from_hex(&c.mask).map_err(|e| anyhow!("coin mask: {e:?}"))?,
        value: c.value,
        commitment: PedersenCommitmentBytes::from_hex(&c.commitment).map_err(|e| anyhow!("commitment: {e:?}"))?,
    })
}

fn keys_from_hex(sk: &str) -> Result<Keys> {
    let sk = RistrettoSecretKey::from_hex(sk).map_err(|e| anyhow!("secret key: {e:?}"))?;
    let pk = ootle_byte_type::ToByteType::to_byte_type(&RistrettoPublicKey::from_secret_key(&sk));
    Ok(Keys { sk, pk })
}

fn actor_to_j(a: &Actor) -> ActorJ {
    ActorJ {
        sk: a.keys.sk.to_hex(),
        fee_coin: coin_to_j(&a.fee_coin),
    }
}

fn actor_from_j(a: &ActorJ) -> Result<Actor> {
    Ok(Actor {
        keys: keys_from_hex(&a.sk)?,
        fee_coin: coin_from_j(&a.fee_coin)?,
    })
}

/// How many coupons the agent spends for `amount` uT: rounded up, at least one.
pub fn coupons_for(amount: u64, coupon_ut: u64) -> u64 {
    amount.div_ceil(coupon_ut.max(1)).max(1)
}

impl State {
    fn load(path: &str) -> Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| format!("read {path}"))?;
        serde_json::from_str(&text).with_context(|| format!("parse {path}"))
    }

    fn save(&self, path: &str) -> Result<()> {
        std::fs::write(path, serde_json::to_string_pretty(self)?).with_context(|| format!("write {path}"))
    }

    fn leash(&self) -> Result<ComponentAddress> {
        self.leash
            .as_deref()
            .context("no leash yet: run `familiar leash <state>` first")?
            .parse()
            .map_err(|e| anyhow!("leash address: {e:?}"))
    }

    fn payee_root(&self, name: &str) -> Option<Hash32> {
        self.payees
            .iter()
            .find(|p| p.name == name)
            .and_then(|p| p.root.parse().ok())
    }

    fn record(&mut self, step: &str, sent: &Sent) {
        self.history.push(serde_json::json!({
            "step": step,
            "tx_id": sent.tx_id,
            "accepted": sent.accepted,
            "reason": sent.reason,
        }));
    }
}

/// The variable if it is set and not empty, else the default.
fn env_or(name: &str, default: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn template_address() -> Result<TemplateAddress> {
    let t = env_or("FAMILIAR_TEMPLATE", DEFAULT_TEMPLATE);
    if t.trim().is_empty() {
        bail!("no template address: set FAMILIAR_TEMPLATE (see README, 'Try it')");
    }
    t.trim().parse().map_err(|e| anyhow!("template address: {e:?}"))
}

fn tap_address() -> Result<ComponentAddress> {
    let t = env_or("FAMILIAR_TAP", DEFAULT_TAP);
    if t.trim().is_empty() {
        bail!("no tap address: set FAMILIAR_TAP (see README, 'Try it')");
    }
    t.trim().parse().map_err(|e| anyhow!("tap address: {e:?}"))
}

fn account_secret(network: Network, v: &str) -> Result<OotleSecretKey> {
    crate::funder_secret(network, v)
}

async fn connect(network: Network, account: &str) -> Result<Prov> {
    let wallet = OotleWallet::from(PrivateKeyProvider::new(account_secret(network, account)?));
    ProviderBuilder::new()
        .wallet(wallet)
        .connect_with_transaction_timeout(default_indexer_url(network), Duration::from_secs(180))
        .await
        .context("connect indexer")
}

async fn max_epoch(p: &Prov) -> Result<Epoch> {
    Ok(Epoch(p.get_epoch().await?.as_u64() + 30))
}

fn say(step: &str, sent: &Sent) {
    let link = if sent.tx_id.is_empty() {
        "(not submitted)".to_string()
    } else {
        format!("{EXPLORER}{}", sent.tx_id)
    };
    if sent.accepted {
        println!("{step}: ACCEPTED by the network  {link}");
    } else {
        println!("{step}: REFUSED by the network: {}  {link}", sent.reason);
    }
}

/// Signs `unsigned` with the account key (dry run first, for the reason), submits it.
async fn send_signed(p: &mut Prov, unsigned: tari_ootle_transaction::UnsignedTransaction) -> Result<Sent> {
    let dry = TransactionRequest::default()
        .with_transaction(unsigned.clone().with_dry_run(true))
        .build(p.wallet())
        .await
        .context("sign (dry run)")?;
    let tx = TransactionRequest::default()
        .with_transaction(unsigned)
        .build(p.wallet())
        .await
        .context("sign")?;
    send(p, tx, dry).await
}

/// `familiar wallet <state>`: a fresh testnet account key, written only to the state file.
pub fn wallet(network: Network, path: &str) -> Result<()> {
    if std::path::Path::new(path).exists() {
        bail!("{path} exists; pick another file name (it may hold a funded wallet)");
    }
    let secret = OotleSecretKey::random(network);
    let state = State {
        account: format!(
            "{}:{}",
            secret.account_secret().to_hex(),
            secret.view_only_secret().to_hex()
        ),
        coupon_ut: COUPON_UT,
        ..State::default()
    };
    state.save(path)?;
    println!("address:  {}", secret.to_address());
    println!("account:  {}", secret.to_address().to_account_address());
    println!("wallet written to {path} (testnet keys; keep the file out of any repository)");
    println!("next: familiar tap {path}");
    Ok(())
}

/// `familiar tap <state>`: takes tTARI from the Familiar tap into the account (created on first use). The fee is paid
/// from what the tap gives, so an empty account works.
pub async fn tap(network: Network, path: &str) -> Result<()> {
    let mut state = State::load(path)?;
    let tap = tap_address()?;
    let mut p = connect(network, &state.account).await?;
    let me = p.default_signer_address().clone();
    let account = me.to_account_address();
    let account_pk = *me.account_public_key();
    let exists = p
        .get_substate(account)
        .await
        .map_err(|e| anyhow!("indexer: {e:?}"))?
        .is_some();
    let mut b = Transaction::builder(p.network(), max_epoch(&p).await?).with_fee_instructions_builder(|f| {
        let f = f
            .call_method(tap, "take", args![])
            .put_last_instruction_output_on_workspace("tap")
            .take_from_bucket("tap", FEE, "fee")
            .pay_fee_from_bucket("fee");
        if exists {
            f.call_method(account, "deposit", args![Workspace("tap")])
        } else {
            f.create_account_with_bucket(account_pk, "tap")
        }
    });
    b = b.with_auto_fill_inputs().add_input(TARI_TOKEN).add_input(tap);
    if exists {
        b = b.add_input(account);
    }
    let want = std::collections::HashSet::from([WantInput::VaultForResource {
        component_address: tap,
        resource_address: TARI_TOKEN,
        required: true,
    }]);
    let unsigned = p
        .resolve_input_want_list(b.build_unsigned(), &want)
        .await
        .map_err(|e| anyhow!("resolve tap vault: {e:?}"))?;
    let sent = send_signed(&mut p, unsigned).await?;
    say("tap", &sent);
    state.record("tap", &sent);
    state.save(path)?;
    if !sent.accepted {
        bail!("tap refused");
    }
    // The indexer may not have the new account yet; the balance is informative only.
    match p.get_account_balance(account, TARI_TOKEN).await {
        Ok(balance) => println!("account balance: {balance} uT"),
        Err(_) => println!("account balance: not readable from the indexer yet (the tap transaction was accepted)"),
    }
    println!("next: familiar leash {path}");
    Ok(())
}

/// `familiar leash <state>`: one-time owner and agent keys funded from the account, a leash in coupon mode with two
/// allow-listed payees, and COUPONS hidden coupons. Five transactions.
pub async fn leash(network: Network, path: &str) -> Result<()> {
    let mut state = State::load(path)?;
    if state.leash.is_some() {
        bail!("{path} already has a leash; use a new wallet file for another one");
    }
    let template = template_address()?;
    let mut p = connect(network, &state.account).await?;
    let max = max_epoch(&p).await?;
    let me = p.default_signer_address().clone();
    let account = me.to_account_address();
    let account_pk = *me.account_public_key();

    // 1. one-time keys: the owner (pays for the coupons) and the agent (pays fees only)
    let owner_keys = new_keys();
    let agent_keys = new_keys();
    let coupons_total = COUPONS as u64 * COUPON_UT;
    let owner_share = coupons_total + OWNER_FEES;
    let total = owner_share + AGENT_FEES + FEE;
    let outs = vec![
        output(owner_share, SpendAuthorization::Key(owner_keys.pk))?,
        output(AGENT_FEES, SpendAuthorization::Key(agent_keys.pk))?,
    ];
    let st = statement(
        vec![],
        total,
        &outs,
        Some(RevealedOutput::new(Amount::from(FEE), account_pk)),
    )?;
    let fee_coins = coins(&st, &outs);
    let unsigned = Transaction::builder(p.network(), max)
        .with_fee_instructions_builder(|f| {
            f.call_method(account, "withdraw", args![TARI_TOKEN, Amount::from(total)])
                .put_last_instruction_output_on_workspace("funds")
                .stealth_transfer_with_input_bucket(TARI_TOKEN, st, "funds")
                .put_last_instruction_output_on_workspace("fees")
                .pay_fee_from_bucket("fees")
        })
        .with_auto_fill_inputs()
        .add_input(TARI_TOKEN)
        .add_input(account)
        .build_unsigned();
    let want = std::collections::HashSet::from([WantInput::VaultForResource {
        component_address: account,
        resource_address: TARI_TOKEN,
        required: true,
    }]);
    let unsigned = p
        .resolve_input_want_list(unsigned, &want)
        .await
        .map_err(|e| anyhow!("resolve account vault: {e:?}"))?;
    let sent = send_signed(&mut p, unsigned).await?;
    say("1/5 one-time owner and agent keys", &sent);
    state.record("keys", &sent);
    if !sent.accepted {
        state.save(path)?;
        bail!("funding the one-time keys was refused (balance too low? run `familiar tap` again)");
    }
    let mut owner = Actor {
        keys: owner_keys,
        fee_coin: fee_coins[0].clone(),
    };
    let agent = Actor {
        keys: agent_keys,
        fee_coin: fee_coins[1].clone(),
    };
    state.owner = Some(actor_to_j(&owner));
    state.agent = Some(actor_to_j(&agent));
    state.template = Some(template.to_string());
    state.coupon_ut = COUPON_UT;
    state.save(path)?;

    // 2. the leash. Amount mode is unused here (no budget root): its limit of 1 uT says nothing about the coupons.
    let payee_keys: Vec<Keys> = PAYEES.iter().map(|_| new_keys()).collect();
    let payee_roots: Vec<Hash32> = payee_keys
        .iter()
        .map(|k| root(&[key_leaf(k.pk)]))
        .collect::<Result<_>>()?;
    let new_args = args![
        TARI_TOKEN,
        agent.keys.pk,
        owner.keys.pk,
        1u64,
        WINDOW_EPOCHS,
        payee_roots.clone()
    ];
    let tx = build_tx(&p, max, &owner, FEE, &[], |b| {
        b.call_function(template, "new", new_args)
    })?;
    let sent = send_from(&mut p, &mut owner, tx).await?;
    say("2/5 create the leash", &sent);
    state.record("create leash", &sent);
    state.owner = Some(actor_to_j(&owner));
    let Some(leash) = sent.component.filter(|_| sent.accepted) else {
        state.save(path)?;
        bail!("create refused: {}", sent.reason);
    };
    state.leash = Some(leash.to_string());
    state.payees = PAYEES
        .iter()
        .zip(&payee_roots)
        .map(|(n, r)| PayeeJ {
            name: n.to_string(),
            root: r.to_string(),
        })
        .collect();
    state.save(path)?;

    // 3. coupon mode: the coupon tree's root and the inclusion proof of its errand leaf, the counts
    let leaves = coupon_tree(leash, owner.keys.pk);
    let (witness, coupon_root) = script_path_witness(&leaves, &leaves[0]).map_err(|e| anyhow!("witness: {e:?}"))?;
    let siblings = witness
        .as_script_path()
        .map(|(_, proof, _)| proof.siblings.clone())
        .context("script path")?;
    let tx = build_tx(&p, max, &owner, FEE, &[], |b| {
        b.add_input(leash).call_method(
            leash,
            "set_coupons",
            args![coupon_root, siblings, PER_WINDOW, WINDOW_EPOCHS, PER_PAYMENT],
        )
    })?;
    let sent = send_from(&mut p, &mut owner, tx).await?;
    say("3/5 coupon mode on", &sent);
    state.record("set coupons", &sent);
    state.owner = Some(actor_to_j(&owner));
    state.save(path)?;
    if !sent.accepted {
        bail!("set_coupons refused: {}", sent.reason);
    }

    // 4. the coupons: COUPONS hidden UTXOs under the coupon tree, from the owner's coin
    let (first, second, new) = fund_coupons(&mut p, max, &mut owner, coupon_root, COUPONS).await?;
    say("4/5 owner sets aside the coupon purse", &first);
    state.record("coupon purse", &first);
    if let Some(second) = &second {
        say("5/5 cut the purse into hidden coupons", second);
        state.record("cut coupons", second);
    }
    state.coupons = new.iter().map(coin_to_j).collect();
    state.owner = Some(actor_to_j(&owner));
    state.save(path)?;
    if new.is_empty() {
        bail!("funding the coupons was refused");
    }
    println!(
        "leash {leash}: {COUPONS} coupons, at most {PER_PAYMENT} per payment and {PER_WINDOW} per window; payees: {}",
        PAYEES.join(", ")
    );
    println!("next: familiar pay {path} candle-shop 2   or   familiar ask {path} \"pay 300000 uT to candle-shop\"");
    Ok(())
}

/// The coupon tree: errand `component(<leash>)` with no covenant, recall `public_key(<owner>)`.
pub fn coupon_tree(
    leash: ComponentAddress,
    owner: tari_template_lib_types::crypto::RistrettoPublicKeyBytes,
) -> Vec<SpendCondition> {
    vec![SpendCondition::access_rule(rule!(component(leash))), key_leaf(owner)]
}

/// Funds `n` coupons of COUPON_UT under `coupon_root` from the owner's key-path coin, in two transactions: the fee
/// intent has a flat compute credit (32M points) that range proofs for 11 outputs exceed, so (1) the fee intent splits
/// off a purse (2 outputs) and (2) the main instructions cut the purse into the coupons. Only fees are revealed.
/// Returns the two outcomes and, when both were accepted, the coupons.
pub async fn fund_coupons(
    p: &mut Prov,
    max: Epoch,
    owner: &mut Actor,
    coupon_root: Hash32,
    n: usize,
) -> Result<(Sent, Option<Sent>, Vec<Coin>)> {
    let purse_value = n as u64 * COUPON_UT;
    let change = owner
        .fee_coin
        .value
        .checked_sub(purse_value + FEE)
        .context("owner coin too small for the coupons")?;
    let outs = vec![
        output(purse_value, SpendAuthorization::Key(owner.keys.pk))?,
        output(change, SpendAuthorization::Key(owner.keys.pk))?,
    ];
    let st = statement(
        vec![StealthInputWitness::new(owner.fee_coin.mav())],
        0,
        &outs,
        Some(RevealedOutput::new(Amount::from(FEE), owner.keys.pk)),
    )?;
    let split = coins(&st, &outs);
    let unsigned = Transaction::builder(p.network(), max)
        .with_fee_instructions_builder(|f| {
            f.stealth_transfer(TARI_TOKEN, st)
                .put_last_instruction_output_on_workspace("fees")
                .pay_fee_from_bucket("fees")
        })
        .with_auto_fill_inputs()
        .add_input(TARI_TOKEN)
        .add_input(owner.fee_coin.address())
        .build_unsigned();
    let dry = unsigned.clone().with_dry_run(true).finish().seal(&owner.keys.sk);
    let tx = unsigned.finish().seal(&owner.keys.sk);
    let first = send(p, tx, dry).await?;
    if !first.accepted {
        return Ok((first, None, Vec::new()));
    }
    let purse = split[0].clone();
    owner.fee_coin = split[1].clone();

    let mut outs = Vec::new();
    for _ in 0..n {
        outs.push(output(COUPON_UT, SpendAuthorization::Script(coupon_root))?);
    }
    let st = statement(vec![StealthInputWitness::new(purse.mav())], 0, &outs, None)?;
    let new = coins(&st, &outs);
    let tx = build_tx(p, max, owner, FEE, &[purse.address()], |b| {
        b.stealth_transfer(TARI_TOKEN, st)
    })?;
    let second = send_from(p, owner, tx).await?;
    let coupons = if second.accepted { new } else { Vec::new() };
    Ok((first, Some(second), coupons))
}

/// The pay_coupons statement: `coupons` spent through the errand leaf without a claim, all of their value in one
/// output to `to`.
pub fn coupon_statement(
    leaves: &[SpendCondition],
    coupons: &[Coin],
    to: SpendAuthorization,
) -> Result<StealthTransferStatement> {
    let (witness, _) = script_path_witness(leaves, &leaves[0]).map_err(|e| anyhow!("witness: {e:?}"))?;
    let value: u64 = coupons.iter().map(|c| c.value).sum();
    let outs = vec![output(value, to)?];
    // No committed root, so the wallet builds no covenant claim: nothing says how much moves.
    let inputs = coupons.iter().map(|c| recall_input(c, witness.clone())).collect();
    statement(inputs, 0, &outs, None)
}

/// The agent pays `n` coupons to `to`. Returns whether the network accepted it.
async fn agent_pays(state: &mut State, path: &str, step: &str, to: Hash32, n: u64) -> Result<bool> {
    let leash = state.leash()?;
    let mut p = connect(Network::Esmeralda, &state.account).await?;
    let max = max_epoch(&p).await?;
    let mut agent = actor_from_j(state.agent.as_ref().context("no agent")?)?;
    let owner = keys_from_hex(&state.owner.as_ref().context("no owner")?.sk)?;
    let all: Vec<Coin> = state.coupons.iter().map(coin_from_j).collect::<Result<_>>()?;
    if all.is_empty() {
        bail!("no coupons left (recalled or spent)");
    }
    // Ask for what the text asked for, even beyond what is left or allowed: the network decides.
    let take = (n as usize).min(all.len());
    let spent = &all[..take];
    let leaves = coupon_tree(leash, owner.pk);
    let st = coupon_statement(&leaves, spent, SpendAuthorization::Script(to))?;
    let addrs: Vec<UtxoAddress> = spent.iter().map(Coin::address).collect();
    let tx = build_tx(&p, max, &agent, FEE, &addrs, |b| {
        b.add_input(leash).call_method(leash, "pay_coupons", args![st])
    })?;
    let sent = send_from(&mut p, &mut agent, tx).await?;
    say(step, &sent);
    state.record(step, &sent);
    state.agent = Some(actor_to_j(&agent));
    if sent.accepted {
        state.coupons.drain(..take);
    }
    state.save(path)?;
    if take < n as usize {
        println!("(asked for {n} coupons, the wallet holds {})", all.len());
    }
    Ok(sent.accepted)
}

/// `familiar pay <state> <payee> <coupons>`.
pub async fn pay(path: &str, payee: &str, n: u64) -> Result<()> {
    let mut state = State::load(path)?;
    let to = match state.payee_root(payee) {
        Some(root) => root,
        None => {
            println!("{payee} is not on the leash: paying a fresh root the leash does not know");
            root(&[key_leaf(new_keys().pk)])?
        }
    };
    agent_pays(&mut state, path, &format!("pay {n} coupon(s) to {payee}"), to, n).await?;
    Ok(())
}

/// `familiar ask <state> <text>`: the obedient agent reads `text` and pays whatever it says.
pub async fn ask(path: &str, text: &str) -> Result<()> {
    let mut state = State::load(path)?;
    let Some(call) = agent::parse(text) else {
        println!("the agent read your text and found no payment instruction (\"pay <N> uT to <name>\")");
        return Ok(());
    };
    let n = coupons_for(call.amount, state.coupon_ut);
    println!(
        "agent: tool call pay {} uT to {} = {n} coupon(s) of {} uT",
        call.amount, call.payee, state.coupon_ut
    );
    let to = match state.payee_root(&call.payee) {
        Some(root) => root,
        None => root(&[key_leaf(new_keys().pk)])?,
    };
    agent_pays(
        &mut state,
        path,
        &format!("agent pays {} uT to {}", call.amount, call.payee),
        to,
        n,
    )
    .await?;
    Ok(())
}

/// `familiar pause|resume <state>`: owner-only calls.
pub async fn owner_call(path: &str, method: &str) -> Result<()> {
    let mut state = State::load(path)?;
    let leash = state.leash()?;
    let mut p = connect(Network::Esmeralda, &state.account).await?;
    let max = max_epoch(&p).await?;
    let mut owner = actor_from_j(state.owner.as_ref().context("no owner")?)?;
    let tx = build_tx(&p, max, &owner, FEE, &[], |b| {
        b.add_input(leash).call_method(leash, method, args![])
    })?;
    let sent = send_from(&mut p, &mut owner, tx).await?;
    say(method, &sent);
    state.record(method, &sent);
    state.owner = Some(actor_to_j(&owner));
    state.save(path)
}

/// `familiar recall <state>`: the owner takes every coupon back through the recall leaf, without a claim, so the
/// amount recalled is not published.
pub async fn recall(path: &str) -> Result<()> {
    let mut state = State::load(path)?;
    let leash = state.leash()?;
    let mut p = connect(Network::Esmeralda, &state.account).await?;
    let max = max_epoch(&p).await?;
    let mut owner = actor_from_j(state.owner.as_ref().context("no owner")?)?;
    let all: Vec<Coin> = state.coupons.iter().map(coin_from_j).collect::<Result<_>>()?;
    if all.is_empty() {
        bail!("no coupons to recall");
    }
    let leaves = coupon_tree(leash, owner.keys.pk);
    let (witness, _) = script_path_witness(&leaves, &leaves[1]).map_err(|e| anyhow!("witness: {e:?}"))?;
    let total: u64 = all.iter().map(|c| c.value).sum();
    let outs = vec![output(total, SpendAuthorization::Key(owner.keys.pk))?];
    let inputs = all.iter().map(|c| recall_input(c, witness.clone())).collect();
    let st = statement(inputs, 0, &outs, None)?;
    let addrs: Vec<UtxoAddress> = all.iter().map(Coin::address).collect();
    let tx = build_tx(&p, max, &owner, FEE, &addrs, |b| b.stealth_transfer(TARI_TOKEN, st))?;
    let sent = send_from(&mut p, &mut owner, tx).await?;
    say("recall", &sent);
    state.record("recall", &sent);
    state.owner = Some(actor_to_j(&owner));
    if sent.accepted {
        state.coupons.clear();
    }
    state.save(path)
}

/// `familiar show <state>`: what this wallet knows, and where to read the leash's public state.
pub fn show(path: &str) -> Result<()> {
    let state = State::load(path)?;
    println!("leash:    {}", state.leash.as_deref().unwrap_or("(none yet)"));
    println!("template: {}", state.template.as_deref().unwrap_or("(none yet)"));
    println!(
        "coupons:  {} left in this wallet, {} uT each (the network never sees this number)",
        state.coupons.len(),
        state.coupon_ut
    );
    println!(
        "payees:   {}",
        state
            .payees
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    if let Some(l) = &state.leash {
        println!("public state of the leash: https://ootle-indexer-a.tari.com/substates/{l}");
    }
    for h in &state.history {
        println!(
            "  {}: {} {}{}",
            h["step"].as_str().unwrap_or(""),
            if h["accepted"].as_bool() == Some(true) {
                "accepted"
            } else {
                "refused"
            },
            h["tx_id"]
                .as_str()
                .map(|t| format!("{EXPLORER}{t}"))
                .unwrap_or_default(),
            h["reason"]
                .as_str()
                .filter(|r| !r.is_empty())
                .map(|r| format!(" ({r})"))
                .unwrap_or_default(),
        );
    }
    Ok(())
}

/// `familiar tap-new <tap.wasm> <amount-uT>` (author only, FAMILIAR_FUNDER): publishes the tap template and creates
/// a tap holding `amount` uT that gives 5 tTARI per take, at most 20 takes per epoch.
pub async fn tap_new(network: Network, wasm_path: &str, amount: u64) -> Result<()> {
    let v = std::env::var("FAMILIAR_FUNDER").context("FAMILIAR_FUNDER not set")?;
    let mut p = connect(network, v.trim()).await?;
    let account = p.default_signer_address().to_account_address();
    let wasm = std::fs::read(wasm_path).context("read tap wasm")?;
    let want = std::collections::HashSet::from([WantInput::VaultForResource {
        component_address: account,
        resource_address: TARI_TOKEN,
        required: true,
    }]);
    let template = match std::env::var("FAMILIAR_TAP_TEMPLATE") {
        Ok(t) if !t.trim().is_empty() => t.trim().parse().map_err(|e| anyhow!("tap template: {e:?}"))?,
        _ => {
            let fee = 3 * TARI;
            let unsigned = Transaction::builder(p.network(), max_epoch(&p).await?)
                .with_fee_instructions_builder(|f| {
                    f.call_method(account, "withdraw", args![TARI_TOKEN, Amount::from(fee)])
                        .put_last_instruction_output_on_workspace("fee")
                        .pay_fee_from_bucket("fee")
                })
                .publish_template(wasm)
                .with_auto_fill_inputs()
                .add_input(TARI_TOKEN)
                .add_input(account)
                .build_unsigned();
            let unsigned = p
                .resolve_input_want_list(unsigned, &want)
                .await
                .map_err(|e| anyhow!("resolve account vault: {e:?}"))?;
            let sent = send_signed(&mut p, unsigned).await?;
            say("publish tap template", &sent);
            let Some(t) = sent.template.filter(|_| sent.accepted) else {
                bail!("publish refused: {}", sent.reason);
            };
            t
        }
    };
    println!("tap template: {template}");
    let unsigned = Transaction::builder(p.network(), max_epoch(&p).await?)
        .with_fee_instructions_builder(|f| {
            f.call_method(account, "withdraw", args![TARI_TOKEN, Amount::from(FEE)])
                .put_last_instruction_output_on_workspace("fee")
                .pay_fee_from_bucket("fee")
        })
        .call_method(account, "withdraw", args![TARI_TOKEN, Amount::from(amount)])
        .put_last_instruction_output_on_workspace("funds")
        .call_function(
            template,
            "new",
            args![Workspace("funds"), Amount::from(5 * TARI), 20u64],
        )
        .with_auto_fill_inputs()
        .add_input(TARI_TOKEN)
        .add_input(account)
        .build_unsigned();
    let unsigned = p
        .resolve_input_want_list(unsigned, &want)
        .await
        .map_err(|e| anyhow!("resolve account vault: {e:?}"))?;
    let sent = send_signed(&mut p, unsigned).await?;
    say("create the tap", &sent);
    let Some(tap) = sent.component.filter(|_| sent.accepted) else {
        bail!("create tap refused: {}", sent.reason);
    };
    println!("TAP={tap}");
    println!("TAP_TEMPLATE={template}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_whole_leash_fits_in_one_take_from_the_tap() {
        let take = 5 * TARI - FEE; // the tap pays its own fee from what it gives
        let needed = COUPONS as u64 * COUPON_UT + OWNER_FEES + AGENT_FEES + FEE;
        assert!(needed <= take, "{needed} > {take}");
    }

    #[test]
    fn built_in_addresses_parse() {
        let t: Result<TemplateAddress, _> = DEFAULT_TEMPLATE.parse();
        assert!(t.is_ok());
        let c: Result<ComponentAddress, _> = DEFAULT_TAP.parse();
        assert!(c.is_ok());
    }

    #[test]
    fn coupons_round_up_and_never_zero() {
        assert_eq!(coupons_for(300_000, 100_000), 3);
        assert_eq!(coupons_for(250_000, 100_000), 3);
        assert_eq!(coupons_for(1, 100_000), 1);
        assert_eq!(coupons_for(0, 100_000), 1);
        assert_eq!(coupons_for(500_000, 100_000), 5);
    }

    #[test]
    fn state_round_trips_and_keeps_coins_spendable() {
        let k = new_keys();
        let coin = Coin {
            mask: k.sk.clone(),
            value: 42,
            commitment: PedersenCommitmentBytes::from([7u8; 32]),
        };
        let j = coin_to_j(&coin);
        let back = coin_from_j(&j).unwrap();
        assert_eq!(back.value, 42);
        assert_eq!(back.mask, coin.mask);
        assert_eq!(back.commitment, coin.commitment);

        let state = State {
            account: "a:b".into(),
            coupon_ut: COUPON_UT,
            coupons: vec![j],
            payees: vec![PayeeJ {
                name: "candle-shop".into(),
                root: Hash32::from([1u8; 32]).to_string(),
            }],
            ..State::default()
        };
        let text = serde_json::to_string(&state).unwrap();
        let again: State = serde_json::from_str(&text).unwrap();
        assert_eq!(again, state);
        assert_eq!(again.payee_root("candle-shop"), Some(Hash32::from([1u8; 32])));
        assert_eq!(again.payee_root("refund-desk"), None);
    }

    #[test]
    fn the_coupon_statement_reveals_no_amount() {
        let leash: ComponentAddress = format!("component_{}", "01".repeat(32)).parse().unwrap();
        let owner = new_keys();
        let leaves = coupon_tree(leash, owner.pk);
        let coupons: Vec<Coin> = (0..3)
            .map(|i| Coin {
                mask: new_keys().sk,
                value: COUPON_UT,
                commitment: PedersenCommitmentBytes::from([i as u8 + 1; 32]),
            })
            .collect();
        let to = SpendAuthorization::Script(root(&[key_leaf(new_keys().pk)]).unwrap());
        let st = coupon_statement(&leaves, &coupons, to).unwrap();
        assert!(st.covenant_claims.is_empty());
        assert!(st.inputs_statement.revealed_amount.is_zero());
        assert!(st.outputs_statement.revealed_output.is_none());
        assert_eq!(st.stealth_inputs().len(), 3);
        assert_eq!(st.stealth_outputs().len(), 1);
        for input in st.stealth_inputs() {
            let (leaf, _, _) = input.witness.as_script_path().unwrap();
            assert_eq!(*leaf, leaves[0]);
        }
    }
}
