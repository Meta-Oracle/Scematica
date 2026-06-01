use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    Json,
};
use base64::Engine;
use std::sync::Arc;
use tracing::{debug, warn};

use crate::{
    facilitator::Facilitator,
    types::{PaymentPayload, PaymentRequired, PaymentRequirements, ResourceInfo, X402_VERSION},
};

pub const PAYMENT_HEADER: &str = "X-Payment";
pub const PAYMENT_RESPONSE_HEADER: &str = "X-Payment-Response";

/// Shared state injected into the middleware via `State`.
#[derive(Clone)]
pub struct PaymentGate {
    pub facilitator: Arc<Facilitator>,
    /// All accepted payment options — served in 402 responses.
    pub requirements: Vec<PaymentRequirements>,
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
                    debug!(payer = ?verify.payer, "Payment verified — proceeding");

                    // Pass through to the actual handler
                    let mut response = next.run(req).await;

                    // Settle asynchronously so the API response is not delayed
                    let fac = gate.facilitator.clone();
                    let p = payload.clone();
                    let r = req_spec.clone();
                    tokio::spawn(async move {
                        let result = fac.settle(&p, &r).await;
                        if !result.success {
                            warn!("Async payment settlement failed: {:?}", result.error);
                        }
                        // Encode settlement result for potential header attachment
                        if let Ok(json) = serde_json::to_string(&result) {
                            let b64 = base64::engine::general_purpose::STANDARD.encode(json);
                            drop(b64); // header can't be set after response sent — log only
                        }
                    });

                    response
                        .headers_mut()
                        .insert(PAYMENT_RESPONSE_HEADER, "payment-verified".parse().unwrap());
                    return Ok(response);
                }
                warn!(reason = ?verify.invalid_reason, "Payment verification failed");
            }
        }
    }

    // No valid payment → 402 with what we accept
    let url = req.uri().to_string();
    let method = req.method().to_string();
    let body = PaymentRequired {
        x402_version: X402_VERSION,
        resource: ResourceInfo {
            url,
            method,
            description: "Scematica Protocol — pay per API call".into(),
        },
        accepts: gate.requirements.clone(),
    };
    Err((
        StatusCode::PAYMENT_REQUIRED,
        Json(serde_json::to_value(&body).unwrap_or_default()),
    ))
}

fn decode_payment_payload(encoded: &str) -> Option<PaymentPayload> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}
