//! `discover`, `delegate` and `pay` — the three verbs that were listed and not built.
//!
//! Each was left as a stub with a stated blocker. Those blockers were real and are addressed
//! here rather than routed around, so what each verb now does is narrower than its name
//! suggests and honest about where it stops.
//!
//! ## The constraint that shapes all three
//!
//! **Omni cannot link `scematica-protocol`.** That is where x402 settlement lives, and it
//! depends on `solana-sdk` — the exact pin the separate workspace exists to keep out. So none
//! of these verbs moves money or opens a socket to a counterparty. They decide, they record,
//! and they hand a request to something that settles.
//!
//! That is not a limitation worked around; it is the right split. The decision is pure and
//! testable offline, and the dangerous half is visible as a separate step somebody has to
//! run. A `pay` that both authorised and settled would make the interesting part untestable
//! and the expensive part invisible.
//!
//! ## Dry run by default, everywhere
//!
//! Same rule as `execute`: the two paths compute the same thing up to the last step, which is
//! exactly why they are not the same keystroke.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use scema_spend::{
    authorise, Amount, Ledger, Receipt, ReceiptOutcome, Settlement, SpendPolicy, SpendRecord,
    SpendRequest, Verdict,
};

/// A capability somebody offers. Read from a catalogue file; never invented.
#[derive(serde::Deserialize, serde::Serialize, Clone, Debug)]
pub struct Offer {
    pub capability: String,
    pub payee: String,
    /// Price in the smallest unit, as the catalogue states it.
    pub price: u128,
    pub asset: String,
    #[serde(default)]
    pub note: String,
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

fn load_policy(path: Option<&PathBuf>) -> Result<SpendPolicy> {
    match path {
        Some(p) => read_json(p),
        // Not an error, and not a permissive default. An absent policy is a policy that
        // permits nothing, and every verb below reports that as its own distinct refusal
        // rather than as a breached limit.
        None => Ok(SpendPolicy::deny_all()),
    }
}

/// `scema discover` — what is on offer, and what this agent is allowed to want.
///
/// The original blocker named two things: the relay's catalogue endpoint, and a policy for
/// which capabilities the agent may want. The second is now `scema-spend`. The first is
/// deliberately *not* an HTTP client — the catalogue is a file, or `-` for stdin, so a relay,
/// a curl pipeline and a hand-written list are all the same input.
///
/// It prints what the catalogue said and marks each offer against the policy. It never
/// filters silently: an offer the agent may not buy is shown and labelled, because "this
/// exists and you are not allowed it" is the useful answer and an empty list is not.
pub fn discover(catalogue: &Path, policy_path: Option<&PathBuf>) -> Result<ExitCode> {
    let offers: Vec<Offer> = if catalogue == Path::new("-") {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut s)?;
        serde_json::from_str(&s).context("parsing the catalogue from stdin")?
    } else {
        read_json(catalogue)?
    };
    let policy = load_policy(policy_path)?;

    if offers.is_empty() {
        println!("The catalogue is empty. That is what it said, not a failure to read it.");
        return Ok(ExitCode::SUCCESS);
    }

    println!("OFFERS  {} in {}", offers.len(), catalogue.display());
    let mut allowed = 0;
    for o in &offers {
        let req = SpendRequest {
            capability: o.capability.clone(),
            payee: o.payee.clone(),
            amount: Amount::new(o.price, &o.asset),
            intent: None,
        };
        let v = authorise(&policy, &Ledger::default(), &req);
        let mark = if v.permits() {
            allowed += 1;
            "MAY BUY "
        } else {
            "REFUSED "
        };
        println!("  {mark} {:<24} {:>12} {}  from {}", o.capability, o.price, o.asset, o.payee);
        if let Verdict::Refused { refusal } = &v {
            println!("             {}", refusal.explain());
        }
        if !o.note.is_empty() {
            println!("             {}", o.note);
        }
    }
    println!();
    println!("{allowed} of {} offer(s) are within the spend policy.", offers.len());
    println!("Nothing was bought and nothing was contacted — this reads a catalogue.");
    if policy_path.is_none() {
        println!("No --policy was given, so nothing is buyable: an absent policy permits none.");
    }
    Ok(ExitCode::SUCCESS)
}

/// `scema pay` — decide whether a spend may happen, and record the decision.
///
/// **It does not settle.** See the module note. With `--commit` it seals a [`SpendRecord`]
/// and prints a settlement request for whatever actually moves money; without it, nothing is
/// written. Either way this process holds no key and opens no socket.
#[allow(clippy::too_many_arguments)]
pub fn pay(
    root: &Path,
    capability: &str,
    payee: &str,
    units: u128,
    asset: &str,
    policy_path: Option<&PathBuf>,
    ledger_path: Option<&PathBuf>,
    intent: Option<&str>,
    commit: bool,
) -> Result<ExitCode> {
    let policy = load_policy(policy_path)?;
    let mut ledger: Ledger = match ledger_path {
        Some(p) if p.exists() => read_json(p)?,
        _ => Ledger::default(),
    };

    let request = SpendRequest {
        capability: capability.to_string(),
        payee: payee.to_string(),
        amount: Amount::new(units, asset),
        intent: intent.map(|s| s.to_string()),
    };

    let verdict = authorise(&policy, &ledger, &request);
    println!("SPEND  {} {} to {payee} for {capability}", units, asset);
    println!("  {}", verdict.headline());
    if ledger.committed() > ledger.spent {
        // What is promised but unresolved, stated wherever a budget figure is. A remaining
        // that silently nets off outstanding authorisations is the figure that was wrong
        // before; showing it is how an operator sees a spend nobody ever reconciled.
        println!(
            "  budget  {} settled + {} outstanding of {}",
            ledger.spent,
            ledger.committed() - ledger.spent,
            policy.total,
        );
    }

    // A refusal is still recorded when committing. The pattern of what an agent *wanted* to
    // buy is exactly what a spend policy is for, and it is invisible if refusals vanish.
    let settlement = match (&verdict, commit) {
        (Verdict::Refused { refusal }, _) => Settlement::Refused { reason: refusal.explain() },
        (Verdict::Allowed { .. }, false) => Settlement::DryRun,
        (Verdict::Allowed { .. }, true) => Settlement::Unknown {
            detail: "authorised and handed off; this runtime does not settle, so whether the \
                     money moved is not something it can observe"
                .into(),
        },
    };

    if !commit {
        println!();
        println!("  Dry run. Nothing was written and nothing was attempted.");
        println!("  `--commit` seals a record and emits a settlement request.");
        return Ok(ExitCode::from(settlement.exit_code()));
    }

    let record = SpendRecord::seal(
        concat!("scema-cli/", env!("CARGO_PKG_VERSION")),
        now(),
        request.clone(),
        verdict.clone(),
        settlement.clone(),
    );
    let dir = root.join("spends");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = dir.join(format!("{}.json", record.id));
    std::fs::write(&path, format!("{}\n", serde_json::to_string_pretty(&record)?))
        .with_context(|| format!("writing {}", path.display()))?;

    println!();
    println!("  sealed {}", path.display());
    println!("  {}", settlement.headline());

    // ── the reservation ──────────────────────────────────────────────────────
    //
    // An authorised spend occupies budget from this moment, not from whenever a receipt turns
    // up. Writing it only at reconciliation is what let two authorisations share one allowance:
    // each read a `remaining` the other had already claimed and neither was wrong to. The write
    // happens after sealing because the record id *is* the reservation key — there is nothing to
    // reserve against until the record exists.
    if verdict.permits() {
        if let Some(lp) = ledger_path {
            if ledger.reserve(&record.id, request.amount.units, record.at) {
                std::fs::write(lp, format!("{}\n", serde_json::to_string_pretty(&ledger)?))
                    .with_context(|| format!("writing {}", lp.display()))?;
                println!(
                    "  reserved {} {} against the budget; {} left",
                    request.amount.units,
                    request.amount.asset,
                    ledger.remaining(&policy),
                );
            } else {
                // The id is a digest of the sealed body, so this means the identical record was
                // authorised before. Refusing is the point: under reservations a replay would
                // take a second hold on the same budget.
                println!("  NOT reserved — this record was already authorised");
            }
        } else {
            // Said plainly, because the cap is inert without it and the command otherwise looks
            // like it enforced one.
            println!("  budget  NOT reserved — no --ledger was given, so nothing accumulates");
        }
    }

    if verdict.permits() {
        println!();
        println!("SETTLEMENT REQUEST — hand this to something that can pay:");
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "capability": request.capability,
                "payee": request.payee,
                // Serialised through `Amount`, not hand-built. A bare `json!` here would emit
                // the units as a JSON number and reintroduce the precision hazard the type
                // exists to prevent — in the one document that leaves this process carrying
                // an instruction to move money.
                "amount": request.amount,
                "intent": request.intent,
                "spend_record": record.id,
            }))?
        );
        println!();
        println!("  The settlement is UNKNOWN until a counterparty reference is recorded.");
        println!("  Unknown is not failed: retrying may pay twice. Reconcile, do not repeat.");
    }
    Ok(ExitCode::from(settlement.exit_code()))
}

/// `scema delegate` — hand a goal to another agent, on the record.
///
/// The original blocker was a bonded result format, so a specialist that answers badly can be
/// slashed rather than merely disbelieved. That bond lives on the ScemaDEX rail and is not in
/// this workspace, so what is built here is the half that can be: a **sealed statement of
/// what was handed off, to whom, and under what spend authority**.
///
/// Without a bond, a delegation is a request rather than a contract, and this says so in the
/// output rather than implying otherwise by staying quiet.
pub fn delegate(
    root: &Path,
    goal: &str,
    to: &str,
    policy_path: Option<&PathBuf>,
    max_units: Option<u128>,
    asset: &str,
    commit: bool,
) -> Result<ExitCode> {
    let policy = load_policy(policy_path)?;
    let units = max_units.unwrap_or(0);

    let request = SpendRequest {
        capability: "delegation".into(),
        payee: to.to_string(),
        amount: Amount::new(units, asset),
        intent: None,
    };
    let verdict = authorise(&policy, &Ledger::default(), &request);

    println!("DELEGATE  \"{goal}\"");
    println!("  to        {to}");
    println!("  budget    {units} {asset}");
    println!("  authority {}", verdict.headline());

    if !verdict.permits() {
        println!();
        println!("  Not delegated. A handoff with no spend authority behind it is a request");
        println!("  the other side has no reason to honour, so it is refused here rather than");
        println!("  sent and quietly ignored.");
        return Ok(ExitCode::from(1));
    }

    if !commit {
        println!();
        println!("  Dry run. Nothing was written and nobody was contacted.");
        return Ok(ExitCode::SUCCESS);
    }

    let record = SpendRecord::seal(
        concat!("scema-cli/", env!("CARGO_PKG_VERSION")),
        now(),
        request,
        verdict,
        Settlement::Unknown {
            detail: format!("delegated to {to}; no bonded result format exists yet, so whether \
                             the work is done or done well is not observable from here"),
        },
    );
    let dir = root.join("delegations");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", record.id));
    std::fs::write(&path, format!("{}\n", serde_json::to_string_pretty(&record)?))?;

    println!();
    println!("  sealed {}", path.display());
    println!();
    println!("  This records what was handed off. It is NOT a contract: without a bond on the");
    println!("  ScemaDEX rail there is nothing to slash, so a specialist that answers badly");
    println!("  can be disbelieved and not penalised. Read the result accordingly.");
    Ok(ExitCode::from(3))
}

/// `scema reconcile` — resolve an `Unknown` spend from a settler's receipt.
///
/// The half that was missing, and its absence was a real defect rather than an omission:
/// `pay` read the ledger but nothing ever wrote it, so the cumulative budget cap was inert
/// across invocations. Every spend saw `spent: 0`. The per-transaction cap worked; the total
/// did not.
///
/// The fix is not "write the ledger in `pay`" — only a **settled** spend may consume budget,
/// and `pay` cannot observe settlement. So the ledger is written here, where a settlement is
/// actually reported, and nowhere else.
///
/// ## The spend record is never edited
///
/// Reconciliation appends its own sealed record naming the spend it resolves. The original
/// stays exactly as sealed, still saying `Unknown`, because that *was* the state of knowledge
/// at that moment and rewriting it would destroy the evidence that the gap existed. Same rule
/// as every other record in this runtime: sealed means sealed.
pub fn reconcile(
    root: &Path,
    receipt_path: &Path,
    ledger_path: Option<&PathBuf>,
) -> Result<ExitCode> {
    let receipt: Receipt = read_json(receipt_path)?;
    if let Err(e) = receipt.validate() {
        bail!("{}", e.explain());
    }

    let spend_path = root.join("spends").join(format!("{}.json", receipt.spend_record));
    let spend: SpendRecord = read_json(&spend_path).with_context(|| {
        format!("no spend record `{}` under {}", receipt.spend_record, root.display())
    })?;

    if !spend.verify() {
        bail!(
            "the spend record `{}` does not match its own commitment — it was edited after              sealing, and reconciling it would launder that",
            spend.id
        );
    }
    if !matches!(spend.settlement, Settlement::Unknown { .. }) {
        println!("SPEND {} is already {}", spend.id, settlement_word(&spend.settlement));
        println!("  Nothing to reconcile. A receipt only resolves an unobserved settlement.");
        return Ok(ExitCode::from(1));
    }

    let mut ledger: Ledger = match ledger_path {
        Some(p) if p.exists() => read_json(p)?,
        _ => Ledger::default(),
    };

    let settlement = match &receipt.outcome {
        ReceiptOutcome::Settled { reference } => {
            Settlement::Settled { reference: reference.clone() }
        }
        ReceiptOutcome::Failed { detail } => Settlement::Failed { detail: detail.clone() },
    };

    println!("RECONCILE  {}", spend.id);
    println!("  {} {} to {}", spend.request.amount.units, spend.request.amount.asset,
             spend.request.payee);
    println!("  receipt from {}", if receipt.settler.is_empty() { "(unnamed settler)" }
             else { &receipt.settler });
    println!("  {}", settlement.headline());

    // An observed failure gives the budget back. This is the ONLY thing that releases a
    // reservation: an unobserved outcome keeps it, because the money may already have moved and
    // a released hold is what lets the retry pay twice.
    if matches!(receipt.outcome, ReceiptOutcome::Failed { .. }) {
        if let Some(p) = ledger_path {
            if ledger.release(&spend.id) {
                std::fs::write(p, format!("{}\n", serde_json::to_string_pretty(&ledger)?))
                    .with_context(|| format!("writing {}", p.display()))?;
                println!("  budget  reservation released; {} outstanding", ledger.reserved.len());
            }
        }
    }

    let mut charged = false;
    if settlement.consumed_budget() {
        if let Some(p) = ledger_path {
            charged = ledger.settle(&spend.id, spend.request.amount.units);
            if charged {
                std::fs::write(p, format!("{}
", serde_json::to_string_pretty(&ledger)?))
                    .with_context(|| format!("writing {}", p.display()))?;
                println!("  budget  {} spent across {} settlement(s)", ledger.spent, ledger.count);
            } else {
                // Not an error. Running this twice is reasonable — after a crash, from a
                // retry loop — and the second run being a no-op is the designed behaviour.
                println!("  budget  already counted; this spend was reconciled before");
            }
        } else {
            println!("  budget  NOT updated — no --ledger was given, so nothing accumulates");
        }
    }

    let record = SpendRecord::seal(
        concat!("scema-cli/", env!("CARGO_PKG_VERSION")),
        now(),
        spend.request.clone(),
        spend.verdict.clone(),
        settlement.clone(),
    );
    let dir = root.join("reconciliations");
    std::fs::create_dir_all(&dir)?;
    let out = dir.join(format!("{}.json", record.id));
    let body = serde_json::json!({
        "reconciles": spend.id,
        "receipt": receipt,
        "record": record,
    });
    std::fs::write(&out, format!("{}
", serde_json::to_string_pretty(&body)?))?;

    println!();
    println!("  sealed {}", out.display());
    println!("  The spend record itself is unchanged and still reads UNKNOWN — that was the");
    println!("  state of knowledge when it was sealed, and rewriting it would erase the gap.");
    if !charged && settlement.consumed_budget() && ledger_path.is_some() {
        return Ok(ExitCode::SUCCESS);
    }
    Ok(ExitCode::from(settlement.exit_code()))
}

fn settlement_word(s: &Settlement) -> &'static str {
    match s {
        Settlement::Settled { .. } => "settled",
        Settlement::Failed { .. } => "failed",
        Settlement::Unknown { .. } => "unknown",
        Settlement::DryRun => "a dry run",
        Settlement::Refused { .. } => "refused",
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "scema-market-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn policy_file(dir: &Path) -> PathBuf {
        let p = dir.join("policy.json");
        std::fs::write(
            &p,
            serde_json::to_string(&SpendPolicy {
                asset: "lamports".into(),
                per_transaction: 1_000,
                total: 10_000,
                capabilities: vec!["inference.rank".into(), "delegation".into()],
                payees: vec!["agent-b".into()],
            })
            .unwrap(),
        )
        .unwrap();
        p
    }

    #[test]
    fn a_dry_run_pay_writes_nothing() {
        let dir = scratch();
        let pol = policy_file(&dir);
        let code = pay(&dir, "inference.rank", "agent-b", 400, "lamports", Some(&pol), None, None, false)
            .unwrap();
        assert_eq!(code, ExitCode::SUCCESS);
        assert!(!dir.join("spends").exists(), "dry run must leave no trace");
    }

    #[test]
    fn committing_seals_a_record_that_verifies() {
        let dir = scratch();
        let pol = policy_file(&dir);
        pay(&dir, "inference.rank", "agent-b", 400, "lamports", Some(&pol), None, Some("abc"), true)
            .unwrap();
        let files: Vec<_> = std::fs::read_dir(dir.join("spends")).unwrap().flatten().collect();
        assert_eq!(files.len(), 1);
        let text = std::fs::read_to_string(files[0].path()).unwrap();
        let rec: SpendRecord = serde_json::from_str(&text).unwrap();
        assert!(rec.verify());
        assert_eq!(rec.request.intent.as_deref(), Some("abc"));
    }

    #[test]
    fn an_authorised_spend_settles_as_unknown_because_this_runtime_cannot_observe_payment() {
        // The honest arm. This process holds no key and opens no socket, so "it worked" is a
        // claim it has no basis for — and `Failed` would invite a retry that pays twice.
        let dir = scratch();
        let pol = policy_file(&dir);
        let code = pay(&dir, "inference.rank", "agent-b", 400, "lamports", Some(&pol), None, None, true)
            .unwrap();
        assert_eq!(code, ExitCode::from(3));
    }

    #[test]
    fn a_refusal_is_still_sealed_when_committing() {
        // What an agent *wanted* to buy is exactly what a spend policy is for, and it is
        // invisible if refusals leave no trace.
        let dir = scratch();
        let pol = policy_file(&dir);
        let code = pay(&dir, "inference.rank", "stranger", 400, "lamports", Some(&pol), None, None, true)
            .unwrap();
        assert_eq!(code, ExitCode::from(1));
        let files: Vec<_> = std::fs::read_dir(dir.join("spends")).unwrap().flatten().collect();
        assert_eq!(files.len(), 1, "the refusal was recorded");
    }

    #[test]
    fn without_a_policy_nothing_is_payable() {
        let dir = scratch();
        let code = pay(&dir, "inference.rank", "agent-b", 1, "lamports", None, None, None, false)
            .unwrap();
        assert_eq!(code, ExitCode::from(1));
    }

    #[test]
    fn discover_shows_refused_offers_rather_than_hiding_them() {
        // "This exists and you are not allowed it" is the useful answer; an empty list is not.
        let dir = scratch();
        let pol = policy_file(&dir);
        let cat = dir.join("catalogue.json");
        std::fs::write(
            &cat,
            serde_json::to_string(&vec![
                Offer { capability: "inference.rank".into(), payee: "agent-b".into(),
                        price: 100, asset: "lamports".into(), note: String::new() },
                Offer { capability: "something.else".into(), payee: "stranger".into(),
                        price: 100, asset: "lamports".into(), note: String::new() },
            ])
            .unwrap(),
        )
        .unwrap();
        assert_eq!(discover(&cat, Some(&pol)).unwrap(), ExitCode::SUCCESS);
    }

    #[test]
    fn an_empty_catalogue_is_reported_as_empty_not_as_an_error() {
        let dir = scratch();
        let cat = dir.join("empty.json");
        std::fs::write(&cat, "[]").unwrap();
        assert_eq!(discover(&cat, None).unwrap(), ExitCode::SUCCESS);
    }

    #[test]
    fn delegation_without_spend_authority_is_refused_rather_than_sent() {
        // A handoff with nothing behind it is a request the other side has no reason to
        // honour. Refusing here beats sending it and being quietly ignored.
        let dir = scratch();
        let code = delegate(&dir, "do the thing", "agent-b", None, Some(10), "lamports", true)
            .unwrap();
        assert_eq!(code, ExitCode::from(1));
        assert!(!dir.join("delegations").exists());
    }

    #[test]
    fn an_authorised_delegation_seals_and_exits_three() {
        // Three, not zero: without a bond there is nothing to slash, so whether the work was
        // done or done well is not observable from here.
        let dir = scratch();
        let pol = policy_file(&dir);
        let code = delegate(&dir, "do it", "agent-b", Some(&pol), Some(500), "lamports", true)
            .unwrap();
        assert_eq!(code, ExitCode::from(3));
        let files: Vec<_> = std::fs::read_dir(dir.join("delegations")).unwrap().flatten().collect();
        assert_eq!(files.len(), 1);
    }


    /// Seal a spend and return its record id.
    fn seal_spend(dir: &Path, pol: &Path) -> String {
        pay(dir, "inference.rank", "agent-b", 400, "lamports", Some(&pol.to_path_buf()), None, None, true)
            .unwrap();
        let f = std::fs::read_dir(dir.join("spends")).unwrap().flatten().next().unwrap();
        f.path().file_stem().unwrap().to_string_lossy().to_string()
    }

    fn write_receipt(dir: &Path, id: &str, body: serde_json::Value) -> PathBuf {
        let p = dir.join("receipt.json");
        let mut v = body;
        v["spend_record"] = serde_json::json!(id);
        std::fs::write(&p, serde_json::to_string(&v).unwrap()).unwrap();
        p
    }

    #[test]
    fn reconciling_a_settlement_charges_the_budget_exactly_once() {
        // The defect this verb exists for: `pay` read the ledger and nothing wrote it, so the
        // cumulative cap was inert. Running twice must be a no-op, not a double charge.
        let dir = scratch();
        let pol = policy_file(&dir);
        let id = seal_spend(&dir, &pol);
        let r = write_receipt(&dir, &id, serde_json::json!({
            "outcome": "settled", "reference": "sig", "settler": "t"
        }));
        let led = dir.join("ledger.json");

        reconcile(&dir, &r, Some(&led)).unwrap();
        let l1: Ledger = serde_json::from_str(&std::fs::read_to_string(&led).unwrap()).unwrap();
        assert_eq!((l1.spent, l1.count), (400, 1));

        reconcile(&dir, &r, Some(&led)).unwrap();
        let l2: Ledger = serde_json::from_str(&std::fs::read_to_string(&led).unwrap()).unwrap();
        assert_eq!((l2.spent, l2.count), (400, 1), "a second run must change nothing");
    }

    #[test]
    fn a_failed_settlement_does_not_charge_the_budget() {
        // Otherwise a counterparty that never delivers still exhausts the allowance.
        let dir = scratch();
        let pol = policy_file(&dir);
        let id = seal_spend(&dir, &pol);
        let r = write_receipt(&dir, &id, serde_json::json!({
            "outcome": "failed", "detail": "counterparty refused"
        }));
        let led = dir.join("ledger.json");
        reconcile(&dir, &r, Some(&led)).unwrap();
        assert!(!led.exists(), "a failure must not write a ledger entry");
    }

    #[test]
    fn the_spend_record_is_never_edited_by_reconciliation() {
        // Sealed means sealed. The original still says UNKNOWN because that was the state of
        // knowledge then, and rewriting it would erase the evidence the gap existed.
        let dir = scratch();
        let pol = policy_file(&dir);
        let id = seal_spend(&dir, &pol);
        let spend_path = dir.join("spends").join(format!("{id}.json"));
        let before = std::fs::read_to_string(&spend_path).unwrap();

        let r = write_receipt(&dir, &id, serde_json::json!({
            "outcome": "settled", "reference": "sig"
        }));
        reconcile(&dir, &r, Some(&dir.join("ledger.json"))).unwrap();

        assert_eq!(std::fs::read_to_string(&spend_path).unwrap(), before);
        assert!(dir.join("reconciliations").exists(), "it appends instead");
    }

    #[test]
    fn an_already_resolved_spend_is_not_reconciled_twice() {
        let dir = scratch();
        let pol = policy_file(&dir);
        // A refused spend is sealed as `Refused`, never `Unknown`.
        pay(&dir, "inference.rank", "stranger", 400, "lamports", Some(&pol), None, None, true).unwrap();
        let f = std::fs::read_dir(dir.join("spends")).unwrap().flatten().next().unwrap();
        let id = f.path().file_stem().unwrap().to_string_lossy().to_string();
        let r = write_receipt(&dir, &id, serde_json::json!({
            "outcome": "settled", "reference": "sig"
        }));
        assert_eq!(reconcile(&dir, &r, None).unwrap(), ExitCode::from(1));
    }

    #[test]
    fn a_tampered_spend_record_is_refused_rather_than_reconciled() {
        // Reconciling an edited record would launder the edit into a fresh sealed artefact.
        let dir = scratch();
        let pol = policy_file(&dir);
        let id = seal_spend(&dir, &pol);
        let spend_path = dir.join("spends").join(format!("{id}.json"));
        let text = std::fs::read_to_string(&spend_path).unwrap();
        std::fs::write(&spend_path, text.replace("\"400\"", "\"999999\"")).unwrap();

        let r = write_receipt(&dir, &id, serde_json::json!({
            "outcome": "settled", "reference": "sig"
        }));
        let e = reconcile(&dir, &r, None).unwrap_err().to_string();
        assert!(e.contains("commitment"), "{e}");
    }

    #[test]
    fn a_settlement_receipt_without_a_reference_is_refused() {
        let dir = scratch();
        let pol = policy_file(&dir);
        let id = seal_spend(&dir, &pol);
        let r = write_receipt(&dir, &id, serde_json::json!({
            "outcome": "settled", "reference": ""
        }));
        assert!(reconcile(&dir, &r, None).is_err());
    }

    #[test]
    fn a_receipt_for_an_unknown_record_names_the_record_it_could_not_find() {
        let dir = scratch();
        let r = write_receipt(&dir, "0000000000000000", serde_json::json!({
            "outcome": "settled", "reference": "sig"
        }));
        let e = reconcile(&dir, &r, None).unwrap_err().to_string();
        assert!(e.contains("0000000000000000"), "{e}");
    }

    #[test]
    fn without_a_ledger_the_budget_is_explicitly_not_updated() {
        // Silently succeeding while accumulating nothing is how the original defect hid.
        let dir = scratch();
        let pol = policy_file(&dir);
        let id = seal_spend(&dir, &pol);
        let r = write_receipt(&dir, &id, serde_json::json!({
            "outcome": "settled", "reference": "sig"
        }));
        reconcile(&dir, &r, None).unwrap();
        assert!(!dir.join("ledger.json").exists());
    }
    #[test]
    fn a_dry_run_delegation_writes_nothing() {
        let dir = scratch();
        let pol = policy_file(&dir);
        delegate(&dir, "do it", "agent-b", Some(&pol), Some(500), "lamports", false).unwrap();
        assert!(!dir.join("delegations").exists());
    }
}
