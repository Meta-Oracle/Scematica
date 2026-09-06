use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    Json,
};
use base64::Engine;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tracing::{debug, warn};

use crate::{
    facilitator::Facilitator,
    types::{PaymentPayload, PaymentRequired, PaymentRequirements, ResourceInfo, X402_VERSION},
};

pub const PAYMENT_HEADER: &str = "X-Payment";
pub const PAYMENT_RESPONSE_HEADER: &str = "X-Payment-Response";

/// How long a spent payment is remembered.
///
/// Must comfortably exceed `max_timeout_seconds` on any served requirement: the window in
/// which a payload is still *valid* is exactly the window in which forgetting it would
/// allow a replay. A blockhash expires in roughly 60–90 seconds, so this is generous.
const SPENT_TTL: Duration = Duration::from_secs(900);

/// The payments this process has already acted on.
///
/// ## Why there is a store at all
///
/// Nothing used to link a payload to a request. One valid `X-Payment` header — captured
/// from a proxy log, a shared client, or the operator's own retry — bought unlimited
/// requests, because the gate's answer did not depend on how many times that payload had
/// already been served. The audit's Lean model states it as
/// `Audit.X402.finding_X02_replay_is_unbounded`: `gateImpl r seen p = gateImpl r seen' p`,
/// for any two histories.
///
/// Keyed by the payer's signature, which is unique per payment: it commits to the
/// message, and the message carries a blockhash the server issued.
///
/// **This is per process.** Two protocol servers behind a load balancer do not share a
/// store and each will honour the same payload once — the same property, and the same
/// honest limit, as the file-backed ledger in `web/lib/scemaworld/treasury.ts`. Bounding
/// it properly needs shared storage, and claiming otherwise here would be worse than
/// saying so.
#[derive(Default)]
struct SpentPayments {
    seen: Mutex<HashMap<String, Instant>>,
}

impl SpentPayments {
    /// Claim `key`. `true` if this caller got it; `false` if it was already spent.
    ///
    /// Test-and-set under one lock: checking and inserting separately is the same
    /// read-decide-write race the treasury path was audited for, and two concurrent
    /// replays of one header would both find it absent.
    fn claim(&self, key: &str) -> bool {
        let now = Instant::now();
        let mut seen = match self.seen.lock() {
            Ok(g) => g,
            // A poisoned lock means a previous holder panicked. Refusing is the safe
            // reading: an unusable replay store must not become an absent one.
            Err(e) => e.into_inner(),
        };
        seen.retain(|_, at| now.duration_since(*at) < SPENT_TTL);
        if seen.contains_key(key) {
            return false;
        }
        seen.insert(key.to_owned(), now);
        true
    }

    /// Give back a claim that turned out not to have been spent.
    ///
    /// Only for a settlement that *observably* did not happen. An error from the send is
    /// different from a send whose result nobody saw — see the `settle` call below.
    fn release(&self, key: &str) {
        if let Ok(mut seen) = self.seen.lock() {
            seen.remove(key);
        }
    }
}

/// Shared state injected into the middleware via `State`.
#[derive(Clone)]
pub struct PaymentGate {
    pub facilitator: Arc<Facilitator>,
    /// All accepted payment options — served in 402 responses.
    pub requirements: Vec<PaymentRequirements>,
    spent: Arc<SpentPayments>,
}

impl PaymentGate {
    pub fn new(facilitator: Arc<Facilitator>, requirements: Vec<PaymentRequirements>) -> Self {
        Self {
            facilitator,
            requirements,
            spent: Arc::new(SpentPayments::default()),
        }
    }
}

/// axum middleware: returns 402 if no valid payment header, otherwise passes through.
pub async fn payment_middleware(
    State(gate): State<Arc<PaymentGate>>,
    req: Request<Body>,
    next: Next<Body>,
) -> Result<axum::response::Response, (StatusCode, Json<serde_json::Value>)> {
    let payment_value = req
        .headers()
        .get(PAYMENT_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_owned());

    if let Some(encoded) = payment_value {
        if let Some(payload) = decode_payment_payload(&encoded) {
            if let Some(req_spec) = gate
                .requirements
                .iter()
                .find(|r| r.network == payload.network && r.scheme == payload.scheme)
            {
                let verify = gate.facilitator.verify(&payload, req_spec);
                if verify.is_valid {
                    // ## Claim the payload before doing anything with it
                    //
                    // The claim is what makes one payment buy one request. It is taken
                    // before settlement rather than after, so two concurrent replays of a
                    // captured header cannot both pass — the loser is refused rather than
                    // queued behind the winner's RPC round trip.
                    let Some(key) = payment_key(&payload) else {
                        warn!("Verified payment carries no signature to key a claim on");
                        return Err(payment_required(&gate, req).await);
                    };
                    if !gate.spent.claim(&key) {
                        warn!(payer = ?verify.payer, "Payment replay refused");
                        return Err(payment_required(&gate, req).await);
                    }

                    // ## Settle, then serve
                    //
                    // This used to serve the resource first and settle in a detached
                    // `tokio::spawn`, discarding the error. Failing to settle therefore
                    // cost the caller nothing — the resource had already been delivered —
                    // and since `verify` was not checking signatures either, a payment
                    // that could never be collected bought the same access as one that
                    // could. Delivery is not undoable, so it has to come second.
                    //
                    // The cost is a confirmation on the request path. That is the honest
                    // price of "paid" meaning paid; a route cheap enough not to want it
                    // should not be behind a payment gate.
                    let result = gate.facilitator.settle(&payload, req_spec).await;
                    if !result.success {
                        // **A failure to settle does not always mean nothing happened.**
                        // `send_and_confirm_transaction` can time out over a transaction
                        // that lands, and releasing the claim there is how a paid request
                        // gets served twice — or worse, paid twice on the caller's retry.
                        // The claim is only given back when the settler reports something
                        // that could not have moved money: a local rejection, before
                        // submission. Anything else keeps the claim.
                        if result.transaction.is_none() {
                            gate.spent.release(&key);
                        }
                        warn!(error = ?result.error, "Payment settlement failed");
                        return Err(payment_required(&gate, req).await);
                    }

                    debug!(payer = ?verify.payer, tx = ?result.transaction, "Payment settled");
                    let mut response = next.run(req).await;
                    let header = serde_json::to_string(&result)
                        .map(|j| base64::engine::general_purpose::STANDARD.encode(j))
                        .unwrap_or_default();
                    if let Ok(v) = header.parse() {
                        response.headers_mut().insert(PAYMENT_RESPONSE_HEADER, v);
                    }
                    return Ok(response);
                }
                warn!(reason = ?verify.invalid_reason, "Payment verification failed");
            }
        }
    }

    Err(payment_required(&gate, req).await)
}

/// The 402, carrying everything a payer needs to build a payment that can be collected.
///
/// `extra.feePayer` and `extra.recentBlockhash` are the server's half of the X-03 fix: the
/// payer signs the message that will actually be submitted, and the facilitator no longer
/// rewrites the blockhash underneath the signature. The blockhash is fetched per response
/// because it expires — serving a cached one would hand out payments that verify and then
/// fail to land, which is the failure this exists to remove.
///
/// If the chain cannot be reached the requirements go out without the block, and a payer
/// then refuses to build rather than building something uncollectible. It is never filled
/// in with a default.
async fn payment_required(
    gate: &PaymentGate,
    req: Request<Body>,
) -> (StatusCode, Json<serde_json::Value>) {
    let context = gate.facilitator.payment_context().await;
    if let Err(e) = &context {
        warn!(error = %e, "Could not fetch a payment context for the 402");
    }
    let accepts = gate
        .requirements
        .iter()
        .cloned()
        .map(|mut r| {
            if let Ok((fee_payer, blockhash)) = &context {
                if !r.extra.is_object() {
                    r.extra = serde_json::json!({});
                }
                if let Some(map) = r.extra.as_object_mut() {
                    map.insert("feePayer".into(), fee_payer.to_string().into());
                    map.insert("recentBlockhash".into(), blockhash.to_string().into());
                }
            }
            r
        })
        .collect();

    let body = PaymentRequired {
        x402_version: X402_VERSION,
        resource: ResourceInfo {
            url: req.uri().to_string(),
            method: req.method().to_string(),
            description: "Scematica Protocol — pay per API call".into(),
        },
        accepts,
    };
    (
        StatusCode::PAYMENT_REQUIRED,
        Json(serde_json::to_value(&body).unwrap_or_default()),
    )
}

/// The key a payment is claimed under: the payer's signature.
///
/// Unique per payment, because the signature commits to a message carrying a blockhash the
/// server issued. Read off the decoded transaction rather than hashed from the header, so
/// two different encodings of one payment cannot both be spent.
fn payment_key(payload: &PaymentPayload) -> Option<String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&payload.payload.transaction)
        .ok()?;
    let tx: solana_sdk::transaction::Transaction = bincode::deserialize(&bytes).ok()?;
    tx.signatures
        .iter()
        .find(|s| **s != solana_sdk::signature::Signature::default())
        .map(|s| s.to_string())
}

fn decode_payment_payload(encoded: &str) -> Option<PaymentPayload> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **X-02.** One payment, one request.
    #[test]
    fn a_payment_can_be_claimed_once() {
        let spent = SpentPayments::default();
        assert!(spent.claim("sig-a"), "a fresh payment is claimable");
        assert!(!spent.claim("sig-a"), "the same payment is not claimable twice");
        assert!(spent.claim("sig-b"), "a different payment is unaffected");
    }

    /// A claim released after an observably-failed settlement can be retaken, so a payer
    /// whose transaction was rejected locally is not billed for a request nobody served.
    #[test]
    fn a_released_claim_can_be_retaken() {
        let spent = SpentPayments::default();
        assert!(spent.claim("sig-a"));
        spent.release("sig-a");
        assert!(spent.claim("sig-a"));
    }

    /// The key is the payer's signature, not the header bytes — so the same payment
    /// cannot be spent twice by re-encoding it.
    #[test]
    fn the_claim_key_is_the_signature() {
        use solana_sdk::{signature::Signature, transaction::Transaction};

        let mut tx = Transaction::default();
        tx.signatures = vec![Signature::default(), Signature::from([9u8; 64])];
        let b64 =
            base64::engine::general_purpose::STANDARD.encode(bincode::serialize(&tx).unwrap());
        let payload = PaymentPayload {
            x402_version: X402_VERSION,
            scheme: "exact".into(),
            network: "solana-mainnet".into(),
            payload: crate::types::SvmExactPayload { transaction: b64 },
        };

        let key = payment_key(&payload).expect("a non-default signature is present");
        assert_eq!(
            key,
            Signature::from([9u8; 64]).to_string(),
            "the all-zero placeholder in the fee-payer slot is not the key"
        );
    }
}
