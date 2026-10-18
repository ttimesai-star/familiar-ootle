//! `familiar scenario <template.wasm> <report.md>`: the whole Familiar story on the Esmeralda testnet, in one run.
//!
//! 1. faucet tTARI into one-time keys for the agent and the owner (fee coins);
//! 2. publish the template, create a leash, set its budget root;
//! 3. fund the budget: stealth UTXOs under `errand = component(leash) AND BalancePreserved(cap)` / `recall = owner`;
//! 4. the agent pays two allow-listed payees;
//! 5. attacks, each submitted for real so the refusal is on-chain: over the per-payment cap, over the window limit,
//!    an unlisted payee, a plain transfer of the budget, a stranger calling `pay`;
//! 6. the owner recalls the rest through the hidden recall leaf.
//!
//! Every key is generated for this run and dropped at the end. Only public keys and transaction ids are written to
//! the report. Testnet only: tTARI has no value.

use std::{fmt::Write as _, time::Duration};

use anyhow::{Context, Result, anyhow, bail};
use ootle_byte_type::ToByteType;
use ootle_rs::{
    Network, ToAccountAddress, TransactionRequest,
    builtin_templates::{UnsignedTransactionBuilder, faucet::IFaucet},
    claim_burn::{ClaimBurn, MinotariBurnClaimProof},
    default_indexer_url,
    key_provider::PrivateKeyProvider,
    keys::OotleSecretKey,
    provider::{IndexerProvider, Provider, ProviderBuilder, WalletProvider},
    stealth::StealthTransfer,
    wallet::OotleWallet,
};
use tari_crypto::{
    keys::{PublicKey as _, SecretKey as _},
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
    tari_utilities::hex::Hex,
};
use tari_ootle_transaction::{Epoch, Transaction, TransactionBuilder, args};
use tari_ootle_wallet_crypto::{
    MaskAndValue, OutputWitness, StealthInputWitness, StealthOutputWitness,
    stealth::{condition_root, create_transfer_statement, script_path_witness},
};
use tari_template_lib_types::{
    Amount, ComponentAddress, EncryptedData, Hash32, TemplateAddress, UtxoAddress,
    constants::{TARI, TARI_TOKEN},
    crypto::{PedersenCommitmentBytes, RistrettoPublicKeyBytes, UtxoTag},
    rule,
    stealth::{
        AtomicCondition, Covenant, RevealedOutput, SpendAuthorization, SpendCondition, StealthTransferStatement,
    },
};

type Prov = IndexerProvider<OotleWallet>;

const EXPLORER: &str = "https://explorer.tari.mw/tx/";
/// Fee revealed by each transaction's fee intent. A stealth-reveal fee keeps no change, so this is the price of a
/// demo transaction, not an estimate; the publish fee is taken from a dry run instead.
const FEE: u64 = 20_000;
const LIMIT: u64 = TARI; // 1 tTARI per window
const WINDOW_EPOCHS: u64 = 10_000;
const MAX_PER_PAYMENT: u64 = 400_000; // 0.4 tTARI per single spend (native covenant)
const BUDGET_UTXO: u64 = 2 * TARI;
/// Default amount one run takes from the funder (override with FAMILIAR_FUND_TOTAL, in uT).
const DEFAULT_FUND_TOTAL: u64 = 30 * TARI;
/// Max fee revealed by the burn claim transaction.
const CLAIM_FEE: u64 = 20_000;

#[derive(Clone)]
struct Coin {
    mask: RistrettoSecretKey,
    value: u64,
    commitment: PedersenCommitmentBytes,
}

impl Coin {
    fn mav(&self) -> MaskAndValue {
        MaskAndValue {
            mask: self.mask.clone(),
            value: self.value,
        }
    }

    fn address(&self) -> UtxoAddress {
        UtxoAddress::new(TARI_TOKEN, self.commitment.into())
    }
}

struct Keys {
    sk: RistrettoSecretKey,
    pk: RistrettoPublicKeyBytes,
}

fn new_keys() -> Keys {
    let sk = RistrettoSecretKey::random(&mut rand::rng());
    let pk = RistrettoPublicKey::from_secret_key(&sk).to_byte_type();
    Keys { sk, pk }
}

fn output(value: u64, auth: SpendAuthorization) -> Result<StealthOutputWitness> {
    let mask = RistrettoSecretKey::random(&mut rand::rng());
    let nonce = RistrettoPublicKey::from_secret_key(&RistrettoSecretKey::random(&mut rand::rng()));
    Ok(StealthOutputWitness {
        witness: OutputWitness {
            amount: value,
            mask,
            sender_public_nonce: nonce,
            minimum_value_promise: 0,
            encrypted_data: EncryptedData::try_from(vec![0u8; EncryptedData::min_size()])
                .map_err(|e| anyhow!("encrypted data: {e:?}"))?,
            resource_view_key: None,
        },
        auth,
        tag: UtxoTag::new(0),
    })
}

fn coins(statement: &StealthTransferStatement, outputs: &[StealthOutputWitness]) -> Vec<Coin> {
    statement
        .outputs_statement
        .outputs
        .iter()
        .zip(outputs)
        .map(|(o, w)| Coin {
            mask: w.witness.mask.clone(),
            value: w.witness.amount,
            commitment: *o.commitment(),
        })
        .collect()
}

fn statement(
    inputs: Vec<StealthInputWitness>,
    revealed_in: u64,
    outputs: &[StealthOutputWitness],
    revealed_out: Option<RevealedOutput>,
) -> Result<StealthTransferStatement> {
    create_transfer_statement(inputs, Amount::from(revealed_in), outputs.iter(), revealed_out)
        .map_err(|e| anyhow!("statement: {e:?}"))
}

/// Fee intent statement: spend a key-path fee coin owned by `who`, reveal `fee` to `who`, keep the change.
fn fee_statement(coin: &Coin, who: &Keys, fee: u64) -> Result<(StealthTransferStatement, Coin)> {
    let change = coin.value.checked_sub(fee).context("fee coin too small")?;
    let outs = vec![output(change, SpendAuthorization::Key(who.pk))?];
    let st = statement(
        vec![StealthInputWitness::new(coin.mav())],
        0,
        &outs,
        Some(RevealedOutput::new(Amount::from(fee), who.pk)),
    )?;
    let change = coins(&st, &outs).remove(0);
    Ok((st, change))
}

struct Actor {
    keys: Keys,
    fee_coin: Coin,
}

struct Tx {
    tx: Transaction,
    dry: Transaction,
    fee_change: Coin,
}

/// A transaction sealed by `actor`, its fee paid from the actor's fee coin, the main intent built by `main`.
fn build_tx(
    p: &Prov,
    max_epoch: Epoch,
    actor: &Actor,
    fee: u64,
    extra_inputs: &[UtxoAddress],
    main: impl FnOnce(TransactionBuilder) -> TransactionBuilder,
) -> Result<Tx> {
    let (fee_st, fee_change) = fee_statement(&actor.fee_coin, &actor.keys, fee)?;
    let mut b = Transaction::builder(p.network(), max_epoch).with_fee_instructions_builder(|f| {
        f.stealth_transfer(TARI_TOKEN, fee_st)
            .put_last_instruction_output_on_workspace("fees")
            .pay_fee_from_bucket("fees")
    });
    b = main(b)
        .with_auto_fill_inputs()
        .add_input(TARI_TOKEN)
        .add_input(actor.fee_coin.address());
    for i in extra_inputs {
        b = b.add_input(i.clone());
    }
    let unsigned = b.build_unsigned();
    Ok(Tx {
        dry: unsigned.clone().with_dry_run(true).finish().seal(&actor.keys.sk),
        tx: unsigned.finish().seal(&actor.keys.sk),
        fee_change,
    })
}

struct Report {
    md: String,
}

impl Report {
    fn row(&mut self, step: &str, sent: &Sent, note: &str) {
        let link = if sent.tx_id.is_empty() {
            "-".to_string()
        } else {
            format!(
                "[{}...]({EXPLORER}{})",
                &sent.tx_id[..12.min(sent.tx_id.len())],
                sent.tx_id
            )
        };
        let reason = if sent.reason.is_empty() {
            String::new()
        } else {
            format!(" - refused: `{}`", sent.reason)
        };
        let _ = writeln!(self.md, "| {step} | {link} | {}{reason} | {note} |", sent.receipt);
        println!("{step}: {} {}{reason} {note}", sent.tx_id, sent.receipt);
    }

    fn section(&mut self, title: &str) {
        let _ = writeln!(
            self.md,
            "\n{title}\n\n| Step | Transaction | Outcome | Note |\n|---|---|---|---|"
        );
    }
}

struct Sent {
    tx_id: String,
    committed_fee: bool,
    accepted: bool,
    receipt: String,
    reason: String,
    template: Option<TemplateAddress>,
    component: Option<ComponentAddress>,
}

fn one_line(s: &str) -> String {
    let s = s.replace(['\n', '|'], " ");
    s.chars().take(300).collect()
}

/// Pulls the human message out of a Debug-printed engine result, e.g. `message: "familiar: ..."`.
fn reason_of(debug: &str) -> String {
    if let Some(i) = debug.find("message: \"") {
        let rest = &debug[i + 10..];
        if let Some(j) = rest.find('"') {
            return one_line(&rest[..j]);
        }
    }
    one_line(debug)
}

/// Dry-runs `tx` (to capture the engine's reason if it is refused), then submits it for real, so that even a refusal
/// leaves a transaction on-chain (the fee intent commits, the rest is rejected).
async fn send(p: &mut Prov, tx: Transaction, dry_tx: Transaction) -> Result<Sent> {
    let dry = match p.send_dry_run(dry_tx).await {
        Ok(r) => format!("{:?}", r.finalize.result),
        Err(e) => format!("dry-run error: {e:?}"),
    };
    let accepted_dry = dry.starts_with("Accept(");
    let pending = p.send_transaction(tx).await.context("submit")?;
    let tx_id = pending.tx_id().to_string();
    let watched = format!("{:?}", pending.watch().await);
    let (receipt, template, component) = match pending.get_receipt().await {
        Ok(rc) => {
            let template = rc
                .diff_summary
                .upped
                .iter()
                .find_map(|s| s.substate_id.as_template())
                .map(|t| t.as_template_address().clone());
            let component = rc
                .diff_summary
                .upped
                .iter()
                .find_map(|s| s.substate_id.as_component_address());
            (format!("{:?}", rc.outcome), template, component)
        }
        Err(e) => (
            format!("no receipt ({})", one_line(&format!("{e:?} / {watched}"))),
            None,
            None,
        ),
    };
    let committed_fee = receipt.contains("Commit");
    let accepted = accepted_dry && receipt == "Commit";
    Ok(Sent {
        tx_id,
        committed_fee,
        accepted,
        receipt,
        reason: if accepted_dry { String::new() } else { reason_of(&dry) },
        template,
        component,
    })
}

/// Sends `tx` from `actor` and keeps the actor's fee coin in step with what the network did.
async fn send_from(p: &mut Prov, actor: &mut Actor, tx: Tx) -> Result<Sent> {
    let sent = send(p, tx.tx, tx.dry).await?;
    if sent.committed_fee {
        actor.fee_coin = tx.fee_change;
    }
    Ok(sent)
}

fn key_leaf(pk: RistrettoPublicKeyBytes) -> SpendCondition {
    SpendCondition::access_rule(rule!(public_key(pk)))
}

fn root(leaves: &[SpendCondition]) -> Result<Hash32> {
    condition_root(leaves).map_err(|e| anyhow!("condition root: {e:?}"))
}

/// The pay statement: spend budget coin `coin` through the errand leaf, `amount` to `to`, change under `change_to`.
fn pay_statement(
    leaves: &[SpendCondition],
    coin: &Coin,
    amount: u64,
    to: SpendAuthorization,
    change_to: SpendAuthorization,
) -> Result<(StealthTransferStatement, Vec<Coin>)> {
    let (witness, budget_root) = script_path_witness(leaves, &leaves[0]).map_err(|e| anyhow!("witness: {e:?}"))?;
    let change = coin.value.checked_sub(amount).context("budget coin too small")?;
    let mut outs = Vec::new();
    if change > 0 {
        outs.push(output(change, change_to)?);
    }
    outs.push(output(amount, to)?);
    let st = statement(
        vec![StealthInputWitness::with_script_path(coin.mav(), witness, budget_root)],
        0,
        &outs,
        None,
    )?;
    let new = coins(&st, &outs);
    Ok((st, new))
}

fn funder_from(network: Network, v: &str) -> Result<PrivateKeyProvider> {
    let (a, b) = v
        .split_once(':')
        .context("FAMILIAR_FUNDER must be <account hex>:<view hex>")?;
    let account = RistrettoSecretKey::from_hex(a).map_err(|e| anyhow!("account secret: {e:?}"))?;
    let view = RistrettoSecretKey::from_hex(b).map_err(|e| anyhow!("view secret: {e:?}"))?;
    Ok(PrivateKeyProvider::new(OotleSecretKey::new(network, account, view)))
}

/// Writes a fresh testnet funder secret to `path` (never to stdout) and prints what is needed to fund it.
fn keygen(network: Network, path: &str) -> Result<()> {
    let secret = OotleSecretKey::random(network);
    let line = format!(
        "{}:{}",
        secret.account_secret().to_hex(),
        secret.view_only_secret().to_hex()
    );
    std::fs::write(path, line).context("write secret file")?;
    let address = secret.to_address();
    let account_pk = RistrettoPublicKey::from_secret_key(secret.account_secret());
    println!("address:            {address}");
    println!("account public key: {}", account_pk.to_hex());
    println!("account component:  {}", address.to_account_address());
    println!("secret written to {path} (testnet only; keep it out of the repository)");
    Ok(())
}

#[derive(serde::Deserialize)]
struct BurnProofFile {
    claim_proof: MinotariBurnClaimProof,
    encrypted_data: EncryptedData,
}

fn not_yet_valid() -> ! {
    println!("NOT_YET_VALID");
    std::process::exit(3)
}

/// Claims an L1 burn (made to the funder's account public key) into the funder's Ootle account. Exits with code 3
/// and prints NOT_YET_VALID while the burn is still inside the confirmation window.
async fn claim(network: Network, proof_path: &str, burned: u64) -> Result<()> {
    let v = std::env::var("FAMILIAR_FUNDER").context("FAMILIAR_FUNDER not set")?;
    let wallet = OotleWallet::from(funder_from(network, v.trim())?);
    let mut p: Prov = ProviderBuilder::new()
        .wallet(wallet)
        .connect_with_transaction_timeout(default_indexer_url(network), Duration::from_secs(180))
        .await
        .context("connect indexer")?;
    let max_epoch = Epoch(p.get_epoch().await?.as_u64() + 30);
    let BurnProofFile {
        claim_proof,
        encrypted_data,
    } = serde_json::from_str(&std::fs::read_to_string(proof_path).context("read proof")?).context("parse proof")?;
    let (unsigned, sealer) = ClaimBurn::new(&p, claim_proof, encrypted_data, max_epoch)
        .with_max_fee(CLAIM_FEE)
        .prepare()
        .await
        .map_err(|e| anyhow!("claim prepare: {e:?}"))?;
    match p.sign_and_send_dry_run_with(&sealer, unsigned.clone()).await {
        Ok(r) => {
            let d = format!("{:?}", r.finalize.result);
            if !d.starts_with("Accept(") {
                if d.contains("NotYetValid") {
                    not_yet_valid();
                }
                bail!("claim dry run refused: {}", reason_of(&d));
            }
        }
        Err(e) => {
            let d = format!("{e:?}");
            if d.contains("NotYetValid") {
                not_yet_valid();
            }
            bail!("claim dry run error: {}", one_line(&d));
        }
    }
    let tx = TransactionRequest::default()
        .with_transaction(unsigned)
        .build(&sealer)
        .await
        .context("seal claim")?;
    let pending = p.send_transaction(tx).await.context("submit claim")?;
    println!("claim tx {}", pending.tx_id());
    let _ = pending.watch().await;
    let rc = pending
        .get_receipt()
        .await
        .map_err(|e| anyhow!("claim receipt: {e:?}"))?;
    println!("claim outcome {:?}", rc.outcome);
    let utxo = rc
        .diff_summary
        .upped
        .iter()
        .find_map(|s| s.substate_id.as_utxo_address())
        .context("no claimed UTXO in the receipt")?;
    let value = burned
        .checked_sub(CLAIM_FEE)
        .context("burn smaller than the claim fee")?;

    // Move the claimed stealth UTXO into the funder's account, which the scenario withdraws from.
    let funder_addr = p.default_signer_address().clone();
    let account_pk = *funder_addr.account_public_key();
    let (st, reqs) = StealthTransfer::new(TARI_TOKEN, &p)
        .spend_stealth_input(funder_addr.clone(), &utxo)
        .to_revealed_output(value)
        .prepare()
        .await
        .map_err(|e| anyhow!("deposit statement: {e:?}"))?;
    let unsigned = Transaction::builder(p.network(), max_epoch)
        .with_fee_instructions_builder(|f| {
            f.stealth_transfer(TARI_TOKEN, st)
                .put_last_instruction_output_on_workspace("all")
                .take_from_bucket("all", FEE, "fee")
                .pay_fee_from_bucket("fee")
                .create_account_with_bucket(account_pk, "all")
        })
        .with_auto_fill_inputs()
        .add_input(TARI_TOKEN)
        .add_input(utxo.clone())
        .build_unsigned();
    let authorizer = p.wallet().stealth_authorizer(reqs);
    let tx = TransactionRequest::default()
        .with_transaction(unsigned)
        .build(&authorizer)
        .await
        .context("seal deposit")?;
    let pending = p.send_transaction(tx).await.context("submit deposit")?;
    println!("deposit tx {}", pending.tx_id());
    let _ = pending.watch().await;
    let rc = pending
        .get_receipt()
        .await
        .map_err(|e| anyhow!("deposit receipt: {e:?}"))?;
    println!(
        "deposit outcome {:?} ({} uT into the funder account)",
        rc.outcome,
        value - FEE
    );
    Ok(())
}

fn fail_report(path: &str, r: &Report, why: String) -> Result<()> {
    std::fs::write(path, &r.md)?;
    bail!(why)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let network = Network::Esmeralda;
    if args.len() == 3 && args[1] == "keygen" {
        return keygen(network, &args[2]);
    }
    if args.len() == 4 && args[1] == "claim" {
        let burned: u64 = args[3].parse().context("burned amount (uT)")?;
        return claim(network, &args[2], burned).await;
    }
    if args.len() != 4 || args[1] != "scenario" {
        bail!(
            "usage: familiar keygen <secret-file> | familiar claim <proof.json> <burned-uT> | familiar scenario <template.wasm> <report.md>"
        );
    }
    let wasm = std::fs::read(&args[2]).context("read wasm")?;
    let report_path = args[3].as_str();

    // FAMILIAR_FUNDER="<account secret hex>:<view secret hex>" selects a persistent testnet funder whose account was
    // topped up beforehand; without it the run tries the shared faucet.
    let (funder, funded_account) = match std::env::var("FAMILIAR_FUNDER") {
        Ok(v) if !v.trim().is_empty() => (funder_from(network, v.trim())?, true),
        _ => (PrivateKeyProvider::random(network), false),
    };
    let wallet = OotleWallet::from(funder);
    let mut p: Prov = ProviderBuilder::new()
        .wallet(wallet)
        .connect_with_transaction_timeout(default_indexer_url(network), Duration::from_secs(180))
        .await
        .context("connect indexer")?;
    let epoch = p.get_epoch().await?.as_u64();
    let max_epoch = Epoch(epoch + 30);

    let mut r = Report { md: String::new() };
    let _ = writeln!(
        r.md,
        "# Familiar on Esmeralda\n\nRun at epoch {epoch}. Testnet only, tTARI has no value. All keys were generated \
         for this run and discarded; only transaction ids are published.\n\nPolicy: limit {LIMIT} uT per \
         {WINDOW_EPOCHS} epochs, at most {MAX_PER_PAYMENT} uT per single spend (native covenant)."
    );
    r.section("**Setup and payments**");

    // ---- 1. faucet into key-path fee coins for the agent, the owner and a stranger
    let agent_keys = new_keys();
    let owner_keys = new_keys();
    let stranger_keys = new_keys();
    let payees = [new_keys(), new_keys()];
    let funder_pk = *p.default_signer_address().account_public_key();
    let fund_total: u64 = match std::env::var("FAMILIAR_FUND_TOTAL") {
        Ok(v) if !v.trim().is_empty() => v.trim().parse().context("FAMILIAR_FUND_TOTAL")?,
        _ => DEFAULT_FUND_TOTAL,
    };
    let agent_share = 20 * FEE;
    let stranger_share = 3 * FEE;
    let owner_share = fund_total
        .checked_sub(FEE + agent_share + stranger_share)
        .context("fund total too small")?;
    let outs = vec![
        output(agent_share, SpendAuthorization::Key(agent_keys.pk))?,
        output(owner_share, SpendAuthorization::Key(owner_keys.pk))?,
        output(stranger_share, SpendAuthorization::Key(stranger_keys.pk))?,
    ];
    let st = statement(
        vec![],
        fund_total,
        &outs,
        Some(RevealedOutput::new(Amount::from(FEE), funder_pk)),
    )?;
    let fee_coins = coins(&st, &outs);
    let unsigned = if funded_account {
        // A persistent testnet funder: its account was topped up beforehand (the shared faucet can run dry).
        let account = p.default_signer_address().to_account_address();
        Transaction::builder(p.network(), max_epoch)
            .with_fee_instructions_builder(|f| {
                f.call_method(account, "withdraw", args![TARI_TOKEN, Amount::from(fund_total)])
                    .put_last_instruction_output_on_workspace("funds")
                    .stealth_transfer_with_input_bucket(TARI_TOKEN, st, "funds")
                    .put_last_instruction_output_on_workspace("fees")
                    .pay_fee_from_bucket("fees")
            })
            .with_auto_fill_inputs()
            .add_input(TARI_TOKEN)
            .add_input(account)
            .build_unsigned()
    } else {
        IFaucet::new(&p, max_epoch)
            .take_faucet_funds()
            .into_stealth_transfer(st)
            .and_pay_fee_from_revealed_output()
            .prepare()
            .await
            .context("faucet prepare")?
    };
    let dry = TransactionRequest::default()
        .with_transaction(unsigned.clone().with_dry_run(true))
        .build(p.wallet())
        .await
        .context("faucet sign (dry run)")?;
    let tx = TransactionRequest::default()
        .with_transaction(unsigned)
        .build(p.wallet())
        .await
        .context("faucet sign")?;
    let sent = send(&mut p, tx, dry).await?;
    r.row(
        "1. faucet to one-time fee coins",
        &sent,
        "agent, owner, stranger keys are fresh, not accounts",
    );
    if !sent.accepted {
        return fail_report(report_path, &r, format!("faucet failed: {}", sent.reason));
    }
    let mut agent = Actor {
        keys: agent_keys,
        fee_coin: fee_coins[0].clone(),
    };
    let mut owner = Actor {
        keys: owner_keys,
        fee_coin: fee_coins[1].clone(),
    };
    let mut stranger = Actor {
        keys: stranger_keys,
        fee_coin: fee_coins[2].clone(),
    };

    // ---- 2. publish (fee from a dry run)
    let probe = build_tx(&p, max_epoch, &owner, 50 * TARI, &[], |b| {
        b.publish_template(wasm.clone())
    })?;
    let dry = p.send_dry_run(probe.dry).await.context("publish dry run")?;
    let required: u64 = format!("{}", dry.finalize.required_fees())
        .parse()
        .context("required fee")?;
    let publish_fee = required + required / 20 + 1;
    let tx = build_tx(&p, max_epoch, &owner, publish_fee, &[], |b| {
        b.publish_template(wasm.clone())
    })?;
    let sent = send_from(&mut p, &mut owner, tx).await?;
    r.row(
        "2. publish template",
        &sent,
        &format!("{} bytes, fee {publish_fee} uT (dry run {required})", wasm.len()),
    );
    let Some(template) = sent.template.filter(|_| sent.accepted) else {
        return fail_report(report_path, &r, format!("publish failed: {}", sent.reason));
    };

    // ---- 3. create the leash and set its budget root
    let payee_roots: Vec<Hash32> = payees.iter().map(|k| root(&[key_leaf(k.pk)])).collect::<Result<_>>()?;
    let new_args = args![
        TARI_TOKEN,
        agent.keys.pk,
        owner.keys.pk,
        LIMIT,
        WINDOW_EPOCHS,
        payee_roots.clone()
    ];
    let tx = build_tx(&p, max_epoch, &owner, FEE, &[], |b| {
        b.call_function(template, "new", new_args)
    })?;
    let sent = send_from(&mut p, &mut owner, tx).await?;
    r.row(
        "3. create leash",
        &sent,
        &format!("template `{template}`; two allow-listed payee roots"),
    );
    let Some(leash) = sent.component.filter(|_| sent.accepted) else {
        return fail_report(report_path, &r, format!("create failed: {}", sent.reason));
    };
    let leaves = vec![
        SpendCondition::all([
            AtomicCondition::AccessRule(rule!(component(leash))),
            AtomicCondition::Covenant(Covenant::BalancePreserved(MAX_PER_PAYMENT)),
        ]),
        key_leaf(owner.keys.pk),
    ];
    let budget_root = root(&leaves)?;
    let budget_auth = SpendAuthorization::Script(budget_root);

    let tx = build_tx(&p, max_epoch, &owner, FEE, &[], |b| {
        b.call_method(leash, "set_budget_root", args![budget_root])
    })?;
    let sent = send_from(&mut p, &mut owner, tx).await?;
    r.row("3b. set budget root (owner)", &sent, &format!("leash `{leash}`"));
    if !sent.accepted {
        return fail_report(report_path, &r, format!("set_budget_root failed: {}", sent.reason));
    }

    // ---- 4. fund the budget: two hidden UTXOs under the leash tree, straight from the owner's fee coin
    let owner_change = owner
        .fee_coin
        .value
        .checked_sub(2 * BUDGET_UTXO + FEE)
        .context("owner coin too small")?;
    let fund_outs = vec![
        output(BUDGET_UTXO, budget_auth.clone())?,
        output(BUDGET_UTXO, budget_auth.clone())?,
        output(owner_change, SpendAuthorization::Key(owner.keys.pk))?,
    ];
    let fund_st = statement(
        vec![StealthInputWitness::new(owner.fee_coin.mav())],
        0,
        &fund_outs,
        Some(RevealedOutput::new(Amount::from(FEE), owner.keys.pk)),
    )?;
    let fund_coins = coins(&fund_st, &fund_outs);
    let tx = Transaction::builder(p.network(), max_epoch)
        .with_fee_instructions_builder(|f| {
            f.stealth_transfer(TARI_TOKEN, fund_st)
                .put_last_instruction_output_on_workspace("fees")
                .pay_fee_from_bucket("fees")
        })
        .with_auto_fill_inputs()
        .add_input(TARI_TOKEN)
        .add_input(owner.fee_coin.address())
        .build_unsigned();
    let dry = tx.clone().with_dry_run(true).finish().seal(&owner.keys.sk);
    let tx = tx.finish().seal(&owner.keys.sk);
    let sent = send(&mut p, tx, dry).await?;
    r.row(
        "4. fund budget (2 hidden UTXOs)",
        &sent,
        "amounts are Pedersen commitments: the budget is not visible",
    );
    if !sent.accepted {
        return fail_report(report_path, &r, format!("funding failed: {}", sent.reason));
    }
    owner.fee_coin = fund_coins[2].clone();
    let mut budget = vec![fund_coins[0].clone(), fund_coins[1].clone()];

    // ---- 5. the agent pays allow-listed payees
    let payee_auth = |i: usize| SpendAuthorization::Script(payee_roots[i]);
    for (i, amount) in [(0usize, 400_000u64), (1, 400_000)] {
        let (st, new) = pay_statement(&leaves, &budget[0], amount, payee_auth(i), budget_auth.clone())?;
        let input = budget[0].address();
        let tx = build_tx(&p, max_epoch, &agent, FEE, &[input], |b| {
            b.call_method(leash, "pay", args![st])
        })?;
        let sent = send_from(&mut p, &mut agent, tx).await?;
        r.row(
            &format!("5. agent pays payee {} {amount} uT", i + 1),
            &sent,
            "the payee is a stealth output under an opaque root",
        );
        if !sent.accepted {
            return fail_report(report_path, &r, format!("pay failed: {}", sent.reason));
        }
        budget[0] = new[0].clone();
    }

    // ---- 6. attacks, each refused on-chain
    r.section("**Try to break it** (each transaction was submitted; the refusal is the network's)");

    // 6a. single spend over the native covenant cap
    let (st, _) = pay_statement(
        &leaves,
        &budget[1],
        MAX_PER_PAYMENT + 10_000,
        payee_auth(0),
        budget_auth.clone(),
    )?;
    let tx = build_tx(&p, max_epoch, &agent, FEE, &[budget[1].address()], |b| {
        b.call_method(leash, "pay", args![st])
    })?;
    let sent = send_from(&mut p, &mut agent, tx).await?;
    r.row(
        "6a. one spend over the per-spend cap",
        &sent,
        "refused by the covenant inside the UTXO",
    );

    // 6b. over the window limit (0.8 of 1.0 already spent)
    let (st, _) = pay_statement(&leaves, &budget[0], 300_000, payee_auth(0), budget_auth.clone())?;
    let tx = build_tx(&p, max_epoch, &agent, FEE, &[budget[0].address()], |b| {
        b.call_method(leash, "pay", args![st])
    })?;
    let sent = send_from(&mut p, &mut agent, tx).await?;
    r.row("6b. over the window limit", &sent, "refused by the leash component");

    // 6c. pay a key that is not on the leash (the agent's own)
    let (st, _) = pay_statement(
        &leaves,
        &budget[0],
        100_000,
        SpendAuthorization::Key(agent.keys.pk),
        budget_auth.clone(),
    )?;
    let tx = build_tx(&p, max_epoch, &agent, FEE, &[budget[0].address()], |b| {
        b.call_method(leash, "pay", args![st])
    })?;
    let sent = send_from(&mut p, &mut agent, tx).await?;
    r.row(
        "6c. pay itself instead of a payee",
        &sent,
        "refused by the leash component",
    );

    // 6d. skip the leash: plain stealth transfer of the budget to the agent
    let (st, _) = pay_statement(
        &leaves,
        &budget[0],
        100_000,
        SpendAuthorization::Key(agent.keys.pk),
        budget_auth.clone(),
    )?;
    let tx = build_tx(&p, max_epoch, &agent, FEE, &[budget[0].address()], |b| {
        b.stealth_transfer(TARI_TOKEN, st)
    })?;
    let sent = send_from(&mut p, &mut agent, tx).await?;
    r.row(
        "6d. spend the budget outside the leash",
        &sent,
        "refused by the engine: the errand leaf needs the leash frame",
    );

    // 6e. a stranger calls pay with a well-formed statement
    let (st, _) = pay_statement(&leaves, &budget[0], 100_000, payee_auth(0), budget_auth.clone())?;
    let tx = build_tx(&p, max_epoch, &stranger, FEE, &[budget[0].address()], |b| {
        b.call_method(leash, "pay", args![st])
    })?;
    let sent = send_from(&mut p, &mut stranger, tx).await?;
    r.row(
        "6e. a stranger calls pay",
        &sent,
        "refused: pay requires the agent's one-time key",
    );

    // ---- 7. what is left in the window still goes through
    r.section("**After the attacks**");
    let (st, new) = pay_statement(&leaves, &budget[0], 200_000, payee_auth(1), budget_auth.clone())?;
    let tx = build_tx(&p, max_epoch, &agent, FEE, &[budget[0].address()], |b| {
        b.call_method(leash, "pay", args![st])
    })?;
    let sent = send_from(&mut p, &mut agent, tx).await?;
    r.row(
        "7. agent pays the last 200 000 uT of the window",
        &sent,
        "window: 1 000 000 of 1 000 000",
    );
    if sent.accepted {
        budget[0] = new[0].clone();
    }

    // ---- 8. owner recall through the hidden leaf
    let (witness, broot) = script_path_witness(&leaves, &leaves[1]).map_err(|e| anyhow!("witness: {e:?}"))?;
    let total: u64 = budget.iter().map(|c| c.value).sum();
    let recall_outs = vec![output(total, SpendAuthorization::Key(owner.keys.pk))?];
    let inputs = budget
        .iter()
        .map(|c| StealthInputWitness::with_script_path(c.mav(), witness.clone(), broot))
        .collect();
    let recall = statement(inputs, 0, &recall_outs, None)?;
    let addrs: Vec<UtxoAddress> = budget.iter().map(Coin::address).collect();
    let tx = build_tx(&p, max_epoch, &owner, FEE, &addrs, |b| {
        b.stealth_transfer(TARI_TOKEN, recall)
    })?;
    let sent = send_from(&mut p, &mut owner, tx).await?;
    r.row(
        "8. owner recalls the rest",
        &sent,
        "the recall leaf was hidden until this moment",
    );

    let _ = writeln!(
        r.md,
        "\nTemplate: `{template}`. Leash: `{leash}`. Budget root: `{budget_root}`. Payee roots: `{}`, `{}`.",
        payee_roots[0], payee_roots[1]
    );
    std::fs::write(report_path, &r.md)?;
    println!("report written to {report_path}");
    Ok(())
}
