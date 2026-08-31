//! Whether the agent may spend, how much, and on what.
//!
//! `scema pay` was left unbuilt with a one-line reason: *a runtime that can spend without a
//! spend policy is a runtime nobody should install.* This is that policy, and it exists
//! before `pay` does rather than alongside it, because the order is the whole point.
//!
//! ## This crate decides. It never settles.
//!
//! There is no wallet here, no signer, no chain and no HTTP. `authorise` returns a
//! [`Verdict`]; something else moves the money. That is not squeamishness — omni **cannot
//! link `scematica-protocol`**, which is where x402 settlement lives, because it depends on
//! `solana-sdk` and that pin is precisely what the separate workspace exists to keep out.
//!
//! The split turns out to be the right shape anyway. The decision is pure, total and
//! testable offline; settlement is I/O against a counterparty. Putting them in one function
//! would make the interesting half untestable and the dangerous half invisible.
//!
//! ## Money is integers
//!
//! Amounts are `u128` in the asset's smallest unit, never floats. The same rule the escrow
//! console states for u64 token balances, and for a sharper reason here: a float that rounds
//! a limit *up* authorises a spend the operator did not. `Amount` carries its asset so two
//! different assets cannot be compared or summed by accident.
//!
//! ## Nothing is authorised by default
//!
//! An empty policy authorises nothing at all. Not "no limits configured, so no limits" —
//! that is the `#[serde(default)]` bool trap this repository already records, where a missing
//! field silently disables a safety feature. A policy with no payees allows no payees.
//!
//! ## Every refusal names the limit it hit
//!
//! Not a boolean. An operator refused by a per-transaction cap and one refused by an
//! exhausted budget need to do completely different things, and "denied" tells them neither.

use serde::{Deserialize, Serialize};

pub mod receipt;
pub mod record;

pub use receipt::{Receipt, ReceiptError, ReceiptOutcome};
pub use record::{Settlement, SpendRecord};


/// `u128` on the wire as a decimal **string**, never a JSON number.
///
/// Two reasons, and the second is the one that forced it. JSON numbers are IEEE-754 doubles
/// to most parsers, so anything past `Number.MAX_SAFE_INTEGER` (~9e15) silently loses
/// precision — the rule the escrow console already states for u64 token balances, and every
/// figure here is a claim about money. And `serde_json` refuses `u128` outright on the way
/// back in, so a record sealed with a bare integer could be written and never re-read.
mod u128_str {
    use serde::{de::Error, Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &u128, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&v.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u128, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(|_| D::Error::custom(format!("`{s}` is not a u128")))
    }
}

/// An amount in the smallest unit of one asset.
///
/// `u128` because money is integers. A float that rounds a limit up authorises a spend the
/// operator did not, and the failure is silent and in the right direction to be expensive.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Amount {
    /// Smallest unit — lamports, wei, micro-USDC. Never a display value.
    #[serde(with = "u128_str")]
    pub units: u128,
    /// Asset identifier, compared verbatim. Two assets are never summed.
    pub asset: String,
}

impl Amount {
    pub fn new(units: u128, asset: impl Into<String>) -> Self {
        Amount { units, asset: asset.into() }
    }

    /// Sum, or `None` when the assets differ.
    ///
    /// `None` rather than a panic or a silent coercion: adding lamports to wei is a bug in
    /// the caller, and a total that quietly happened is worse than an error.
    pub fn checked_add(&self, other: &Amount) -> Option<Amount> {
        if self.asset != other.asset {
            return None;
        }
        Some(Amount { units: self.units.checked_add(other.units)?, asset: self.asset.clone() })
    }
}

/// What is being bought, from whom.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpendRequest {
    /// Capability being purchased, e.g. `inference.rank`. Matched verbatim against the
    /// policy's allow-list; no globs, no prefixes.
    pub capability: String,
    /// Who is being paid. Verbatim, for the same reason.
    pub payee: String,
    pub amount: Amount,
    /// The decision this spend serves, if any. A spend with no intent is allowed — an
    /// operator may pay for something directly — but it is recorded as having none rather
    /// than being attributed to whatever decision happened to be latest.
    pub intent: Option<String>,
}

/// Caps and allow-lists. An empty policy authorises nothing.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SpendPolicy {
    /// Asset every limit below is denominated in. A request in another asset is refused
    /// rather than converted — this crate holds no prices and must never appear to.
    pub asset: String,
    /// Largest single spend.
    #[serde(with = "u128_str")]
    pub per_transaction: u128,
    /// Largest total across the whole budget period.
    #[serde(with = "u128_str")]
    pub total: u128,
    /// Capabilities this agent may buy. Empty means none.
    pub capabilities: Vec<String>,
    /// Payees this agent may pay. Empty means none.
    pub payees: Vec<String>,
}

impl SpendPolicy {
    /// A policy that permits nothing, which is what an absent configuration means.
    pub fn deny_all() -> Self {
        SpendPolicy::default()
    }

    pub fn permits_capability(&self, c: &str) -> bool {
        self.capabilities.iter().any(|x| x == c)
    }

    pub fn permits_payee(&self, p: &str) -> bool {
        self.payees.iter().any(|x| x == p)
    }
}

/// What has already been spent under a policy.
///
/// Only a **settled** spend appears here. An authorised spend that failed, or one whose
/// settlement could not be observed, does not consume budget — otherwise a flaky counterparty
/// exhausts an allowance without ever delivering.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Ledger {
    #[serde(with = "u128_str")]
    pub spent: u128,
    pub count: usize,
    /// Ids of the spend records already counted.
    ///
    /// Membership rather than a flag, so double-counting is **structurally impossible** and
    /// not merely guarded by a caller remembering to check. Reconciliation is exactly the
    /// operation somebody runs twice — after a crash, from a retry loop, from two terminals —
    /// and a budget that can be charged twice for one payment is worse than no budget.
    #[serde(default)]
    pub settled_ids: Vec<String>,
}

impl Ledger {
    /// Count a settled spend against the budget.
    ///
    /// Returns `false` and changes nothing if this record was already counted. The caller
    /// should report that as "already reconciled" rather than as an error: running it twice
    /// is a reasonable thing to do and the second run is simply a no-op.
    pub fn settle(&mut self, record_id: &str, amount: u128) -> bool {
        if self.settled_ids.iter().any(|x| x == record_id) {
            return false;
        }
        self.spent = self.spent.saturating_add(amount);
        self.count += 1;
        self.settled_ids.push(record_id.to_string());
        true
    }

    pub fn has_settled(&self, record_id: &str) -> bool {
        self.settled_ids.iter().any(|x| x == record_id)
    }

    pub fn remaining(&self, policy: &SpendPolicy) -> u128 {
        policy.total.saturating_sub(self.spent)
    }
}

/// Why a spend was refused. Each names the limit that stopped it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum Refusal {
    /// The policy is empty. Distinct from every other refusal: nothing is configured, rather
    /// than something being configured and exceeded.
    NoPolicy,
    WrongAsset { policy: String, requested: String },
    CapabilityNotAllowed { capability: String },
    PayeeNotAllowed { payee: String },
    OverPerTransaction {
        #[serde(with = "u128_str")]
        limit: u128,
        #[serde(with = "u128_str")]
        requested: u128,
    },
    OverBudget {
        #[serde(with = "u128_str")]
        remaining: u128,
        #[serde(with = "u128_str")]
        requested: u128,
    },
    /// A spend of nothing is not a spend. Refused rather than treated as free, because it is
    /// almost always a caller that failed to fill in an amount.
    ZeroAmount,
}

impl Refusal {
    pub fn explain(&self) -> String {
        match self {
            Refusal::NoPolicy =>
                "no spend policy is configured, so nothing is authorised — an absent policy \
                 permits nothing rather than everything".into(),
            Refusal::WrongAsset { policy, requested } =>
                format!("the policy is denominated in {policy} and the request is in {requested}; \
                         this runtime holds no prices and will not convert"),
            Refusal::CapabilityNotAllowed { capability } =>
                format!("`{capability}` is not in the policy's capability list"),
            Refusal::PayeeNotAllowed { payee } =>
                format!("`{payee}` is not in the policy's payee list"),
            Refusal::OverPerTransaction { limit, requested } =>
                format!("{requested} exceeds the per-transaction limit of {limit}"),
            Refusal::OverBudget { remaining, requested } =>
                format!("{requested} exceeds the {remaining} remaining in the budget"),
            Refusal::ZeroAmount =>
                "a spend of zero is not a spend; this is almost always an unfilled amount".into(),
        }
    }
}

/// The outcome of asking whether a spend may happen.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum Verdict {
    /// Permitted. **Not** an instruction to pay — see the crate note. Something else settles.
    Allowed {
        #[serde(with = "u128_str")]
        remaining_after: u128,
    },
    Refused { refusal: Refusal },
}

impl Verdict {
    pub fn permits(&self) -> bool {
        matches!(self, Verdict::Allowed { .. })
    }

    pub fn headline(&self) -> String {
        match self {
            Verdict::Allowed { remaining_after } => {
                format!("allowed — {remaining_after} would remain in budget")
            }
            Verdict::Refused { refusal } => format!("refused: {}", refusal.explain()),
        }
    }
}

/// Decide whether one spend is permitted.
///
/// Checks run cheapest-and-most-categorical first: whether a policy exists at all, then the
/// asset, then the allow-lists, then the amounts. That order means an operator with a
/// misconfigured policy is told *that* rather than being told they are over budget, which is
/// true but useless.
pub fn authorise(policy: &SpendPolicy, ledger: &Ledger, req: &SpendRequest) -> Verdict {
    let refuse = |refusal| Verdict::Refused { refusal };

    // An empty policy is not a permissive one. This is the first check because every other
    // refusal below would be technically correct and would send the operator to the wrong
    // place — "over budget" when the real answer is "you configured nothing".
    if policy.capabilities.is_empty() && policy.payees.is_empty() && policy.total == 0 {
        return refuse(Refusal::NoPolicy);
    }
    if policy.asset != req.amount.asset {
        return refuse(Refusal::WrongAsset {
            policy: policy.asset.clone(),
            requested: req.amount.asset.clone(),
        });
    }
    if req.amount.units == 0 {
        return refuse(Refusal::ZeroAmount);
    }
    if !policy.permits_capability(&req.capability) {
        return refuse(Refusal::CapabilityNotAllowed { capability: req.capability.clone() });
    }
    if !policy.permits_payee(&req.payee) {
        return refuse(Refusal::PayeeNotAllowed { payee: req.payee.clone() });
    }
    if req.amount.units > policy.per_transaction {
        return refuse(Refusal::OverPerTransaction {
            limit: policy.per_transaction,
            requested: req.amount.units,
        });
    }
    let remaining = ledger.remaining(policy);
    if req.amount.units > remaining {
        return refuse(Refusal::OverBudget { remaining, requested: req.amount.units });
    }
    Verdict::Allowed { remaining_after: remaining - req.amount.units }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> SpendPolicy {
        SpendPolicy {
            asset: "lamports".into(),
            per_transaction: 1_000,
            total: 10_000,
            capabilities: vec!["inference.rank".into()],
            payees: vec!["agent-b".into()],
        }
    }

    fn req(units: u128) -> SpendRequest {
        SpendRequest {
            capability: "inference.rank".into(),
            payee: "agent-b".into(),
            amount: Amount::new(units, "lamports"),
            intent: None,
        }
    }

    #[test]
    fn a_permitted_spend_reports_what_would_remain() {
        let v = authorise(&policy(), &Ledger::default(), &req(400));
        assert_eq!(v, Verdict::Allowed { remaining_after: 9_600 });
    }

    #[test]
    fn an_empty_policy_authorises_nothing() {
        // Not "no limits configured, so no limits". The `#[serde(default)]` bool trap this
        // repository already records: a missing field must not silently disable a guard.
        let v = authorise(&SpendPolicy::deny_all(), &Ledger::default(), &req(1));
        assert_eq!(v, Verdict::Refused { refusal: Refusal::NoPolicy });
    }

    #[test]
    fn an_unconfigured_policy_says_so_rather_than_reporting_a_breached_limit() {
        // Every other refusal would be technically true and would send the operator to
        // raise a limit that was never the problem.
        match authorise(&SpendPolicy::deny_all(), &Ledger::default(), &req(999_999)) {
            Verdict::Refused { refusal } => {
                assert_eq!(refusal, Refusal::NoPolicy);
                assert!(refusal.explain().contains("permits nothing"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_different_asset_is_refused_and_never_converted() {
        // This crate holds no prices and must never appear to. Converting would be inventing
        // an exchange rate inside an authorisation check.
        let mut r = req(10);
        r.amount.asset = "wei".into();
        match authorise(&policy(), &Ledger::default(), &r) {
            Verdict::Refused { refusal: Refusal::WrongAsset { .. } } => {}
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_unlisted_capability_or_payee_is_refused_by_name() {
        let mut c = req(10);
        c.capability = "inference.everything".into();
        assert!(matches!(
            authorise(&policy(), &Ledger::default(), &c),
            Verdict::Refused { refusal: Refusal::CapabilityNotAllowed { .. } }
        ));

        let mut p = req(10);
        p.payee = "somebody-else".into();
        assert!(matches!(
            authorise(&policy(), &Ledger::default(), &p),
            Verdict::Refused { refusal: Refusal::PayeeNotAllowed { .. } }
        ));
    }

    #[test]
    fn allow_lists_are_exact_never_prefixes() {
        // A prefix match on `inference.` would authorise every capability somebody later
        // named under it, including ones that did not exist when the policy was written.
        let mut r = req(10);
        r.capability = "inference.rank.extra".into();
        assert!(!authorise(&policy(), &Ledger::default(), &r).permits());
        r.capability = "inference".into();
        assert!(!authorise(&policy(), &Ledger::default(), &r).permits());
    }

    #[test]
    fn the_per_transaction_cap_binds_even_with_budget_left() {
        match authorise(&policy(), &Ledger::default(), &req(5_000)) {
            Verdict::Refused { refusal: Refusal::OverPerTransaction { limit, requested } } => {
                assert_eq!((limit, requested), (1_000, 5_000));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_exhausted_budget_refuses_a_spend_that_is_individually_fine() {
        let ledger = Ledger { spent: 9_800, count: 20, ..Default::default() };
        match authorise(&policy(), &ledger, &req(500)) {
            Verdict::Refused { refusal: Refusal::OverBudget { remaining, .. } } => {
                assert_eq!(remaining, 200);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn spending_exactly_the_remainder_is_allowed_and_leaves_zero() {
        let ledger = Ledger { spent: 9_500, count: 1, ..Default::default() };
        assert_eq!(
            authorise(&policy(), &ledger, &req(500)),
            Verdict::Allowed { remaining_after: 0 }
        );
    }

    #[test]
    fn a_zero_spend_is_refused_rather_than_treated_as_free() {
        // Almost always a caller that failed to fill in an amount, and letting it through
        // would record a settlement that bought nothing.
        assert!(matches!(
            authorise(&policy(), &Ledger::default(), &req(0)),
            Verdict::Refused { refusal: Refusal::ZeroAmount }
        ));
    }

    #[test]
    fn only_settled_spends_consume_budget() {
        // A spend that was authorised and then failed must not consume the allowance, or a
        // flaky counterparty exhausts it without ever delivering.
        let mut l = Ledger::default();
        assert_eq!(l.remaining(&policy()), 10_000);
        assert!(l.settle("rec-1", 400));
        assert_eq!(l.remaining(&policy()), 9_600);
        assert_eq!(l.count, 1);
    }

    #[test]
    fn one_payment_cannot_be_charged_to_the_budget_twice() {
        // Reconciliation is exactly the operation somebody runs twice — after a crash, from a
        // retry loop, from two terminals. Membership makes the second run a no-op rather than
        // a double charge, which is a property of the type and not of the caller's care.
        let mut l = Ledger::default();
        assert!(l.settle("rec-1", 400));
        assert!(!l.settle("rec-1", 400), "the second call must change nothing");
        assert_eq!(l.spent, 400);
        assert_eq!(l.count, 1);
        assert!(l.has_settled("rec-1"));
    }

    #[test]
    fn a_ledger_written_before_ids_existed_still_loads() {
        // `settled_ids` is `#[serde(default)]` so an older ledger keeps its totals rather
        // than failing to parse — the numbers are the part that must not be lost.
        let l: Ledger = serde_json::from_str(r#"{"spent":"400","count":1}"#).unwrap();
        assert_eq!(l.spent, 400);
        assert!(l.settled_ids.is_empty());
    }

    #[test]
    fn amounts_of_different_assets_are_never_summed() {
        let a = Amount::new(1, "lamports");
        let b = Amount::new(1, "wei");
        assert!(a.checked_add(&b).is_none(), "adding lamports to wei is a caller bug");
        assert_eq!(a.checked_add(&a).unwrap().units, 2);
    }

    #[test]
    fn an_amount_that_would_overflow_is_none_rather_than_wrapping() {
        let a = Amount::new(u128::MAX, "lamports");
        assert!(a.checked_add(&Amount::new(1, "lamports")).is_none());
    }

    #[test]
    fn money_is_a_string_on_the_wire_and_survives_past_2_to_the_53() {
        // JSON numbers are doubles to most parsers, so anything past ~9e15 loses precision
        // silently — the rule the escrow console already states for u64 balances, and every
        // figure here is a claim about money. `serde_json` also refuses a bare `u128` on the
        // way back in, so a record sealed as an integer could be written and never re-read.
        let big = 340_282_366_920_938_463_463_374_607_431_768_211_455u128; // u128::MAX
        let a = Amount::new(big, "wei");
        let text = serde_json::to_string(&a).unwrap();
        assert!(text.contains("\"340282366920938463463374607431768211455\""), "{text}");
        assert_eq!(serde_json::from_str::<Amount>(&text).unwrap().units, big);
    }

    #[test]
    fn a_policy_round_trips_through_json_unchanged() {
        let p = policy();
        let back: SpendPolicy = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back.total, p.total);
        assert_eq!(back.per_transaction, p.per_transaction);
        assert_eq!(back.payees, p.payees);
    }

    #[test]
    fn a_verdict_carrying_amounts_round_trips_too() {
        // `Verdict` and `Refusal` are inside a sealed record, so a bare u128 there would make
        // the record unreadable — which is how this was found.
        for v in [
            Verdict::Allowed { remaining_after: u128::MAX },
            Verdict::Refused { refusal: Refusal::OverBudget { remaining: 1, requested: 2 } },
            Verdict::Refused {
                refusal: Refusal::OverPerTransaction { limit: 1, requested: u128::MAX },
            },
        ] {
            let text = serde_json::to_string(&v).unwrap();
            assert_eq!(serde_json::from_str::<Verdict>(&text).unwrap(), v, "{text}");
        }
    }

    #[test]
    fn every_refusal_names_the_limit_it_hit() {
        for r in [
            Refusal::NoPolicy,
            Refusal::WrongAsset { policy: "a".into(), requested: "b".into() },
            Refusal::CapabilityNotAllowed { capability: "c".into() },
            Refusal::PayeeNotAllowed { payee: "p".into() },
            Refusal::OverPerTransaction { limit: 1, requested: 2 },
            Refusal::OverBudget { remaining: 1, requested: 2 },
            Refusal::ZeroAmount,
        ] {
            assert!(r.explain().len() > 25, "a refusal nobody can act on: {r:?}");
        }
    }
}
