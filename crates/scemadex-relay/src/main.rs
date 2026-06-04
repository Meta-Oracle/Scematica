//! ScemaDEX peer-mesh relay + signal oracle.
//!
//! Implements the HTTP contract `scemadex_sdk::net::RemotePeerMarket` speaks, so
//! agents can gossip bonded inference offers and learned experience through a
//! shared backend. Also serves the bot's signals (reputation / pool-score /
//! advice) — optionally **x402-gated** for pay-per-call monetization.
//!
//! ```text
//! POST /inference/offer    InferenceOffer            -> 204
//! POST /inference/quote    {intent_digest}           -> InferenceOffer | 404
//! POST /experience/offer   ExperienceBatch           -> 204
//! POST /experience/buy     {max_price}               -> ExperienceBatch | 404
//! GET  /signal/reputation/:mint                      -> Reputation   (x402-gated)
//! GET  /signal/pool_score/:mint                      -> PoolScore    (x402-gated)
//! GET  /signal/advice/:mint                          -> Advice       (x402-gated)
//! GET  /health                                       -> "ok"
//! ```

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    middleware::from_fn_with_state,
    routing::{get, post},
    Json, Router,
};
use clap::Parser;
use serde::Deserialize;

use scemadex_integrations::signal::FileSignalSource;
use scemadex_sdk::{Address, ExperienceBatch, InferenceOffer, SignalSource, Usdc};

use scematica_protocol::middleware::payment_middleware;
use scematica_protocol::{Facilitator, PaymentGate, PaymentRequirements};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::signature::read_keypair_file;

const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

#[derive(Parser, Debug)]
#[command(name = "scemadex-relay", about = "ScemaDEX peer-mesh relay + signal oracle")]
struct Cli {
    /// Port to bind.
    #[arg(long, default_value_t = 8080)]
    port: u16,
    /// Directory where the bot writes its artifacts (reputation, advice).
    #[arg(long, default_value = ".")]
    signal_dir: String,
    /// Enable x402 gating on /signal/* by paying this wallet.
    #[arg(long)]
    pay_to: Option<String>,
    /// Fee-payer keypair file (required for x402 settlement).
    #[arg(long)]
    keypair: Option<String>,
    /// RPC endpoint for settlement.
    #[arg(long)]
    rpc_url: Option<String>,
    /// Price per signal call, in USDC.
    #[arg(long, default_value_t = 0.001)]
    price_usdc: f64,
}

#[derive(Clone)]
struct AppState {
    offers: Arc<Mutex<Vec<InferenceOffer>>>,
    experience: Arc<Mutex<Vec<ExperienceBatch>>>,
    signal: Arc<FileSignalSource>,
}

#[derive(Deserialize)]
struct QuoteReq {
    intent_digest: String,
}

#[derive(Deserialize)]
struct BuyReq {
    max_price: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_target(false).init();
    let cli = Cli::parse();

    let state = AppState {
        offers: Arc::new(Mutex::new(Vec::new())),
        experience: Arc::new(Mutex::new(Vec::new())),
        signal: Arc::new(FileSignalSource::new(&cli.signal_dir)),
    };

    let mut signal_routes = Router::new()
        .route("/signal/reputation/:mint", get(get_reputation))
        .route("/signal/pool_score/:mint", get(get_pool_score))
        .route("/signal/advice/:mint", get(get_advice));

    // Optional x402 gate on the signal endpoints.
    if let Some(gate) = build_gate(&cli)? {
        tracing::info!("x402 gating enabled on /signal/* — {:.4} USDC/call", cli.price_usdc);
        signal_routes = signal_routes.route_layer(from_fn_with_state(Arc::new(gate), payment_middleware));
    } else {
        tracing::info!("/signal/* served open (no --pay-to/--keypair/--rpc-url)");
    }

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/inference/offer", post(post_offer))
        .route("/inference/quote", post(post_quote))
        .route("/experience/offer", post(post_exp_offer))
        .route("/experience/buy", post(post_exp_buy))
        .merge(signal_routes)
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], cli.port));
    tracing::info!("scemadex-relay listening on http://{addr}");
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await?;
    Ok(())
}

fn build_gate(cli: &Cli) -> Result<Option<PaymentGate>> {
    let (Some(pay_to), Some(kp_path), Some(rpc_url)) =
        (cli.pay_to.clone(), cli.keypair.clone(), cli.rpc_url.clone())
    else {
        return Ok(None);
    };
    let keypair = read_keypair_file(&kp_path).map_err(|e| anyhow!("read keypair {kp_path}: {e}"))?;
    let rpc = Arc::new(RpcClient::new(rpc_url));
    let facilitator = Arc::new(Facilitator::new(Arc::new(keypair), rpc, "solana-mainnet"));
    let requirements = vec![PaymentRequirements {
        scheme: "exact".to_string(),
        network: "solana-mainnet".to_string(),
        asset: USDC_MINT.to_string(),
        amount: Usdc::from_usdc(cli.price_usdc).0,
        pay_to,
        max_timeout_seconds: 120,
        extra: serde_json::json!({ "purpose": "scemadex-signal" }),
    }];
    Ok(Some(PaymentGate {
        facilitator,
        requirements,
    }))
}

// ── Mesh endpoints ──────────────────────────────────────────────────────────

async fn post_offer(State(s): State<AppState>, Json(offer): Json<InferenceOffer>) -> StatusCode {
    if let Ok(mut offers) = s.offers.lock() {
        offers.push(offer);
        StatusCode::NO_CONTENT
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    }
}

async fn post_quote(
    State(s): State<AppState>,
    Json(req): Json<QuoteReq>,
) -> Result<Json<InferenceOffer>, StatusCode> {
    let mut offers = s.offers.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let idx = offers
        .iter()
        .enumerate()
        .filter(|(_, o)| o.solution.intent_digest == req.intent_digest)
        .min_by_key(|(_, o)| o.price.0)
        .map(|(i, _)| i);
    match idx {
        Some(i) => Ok(Json(offers.remove(i))),
        None => Err(StatusCode::NOT_FOUND),
    }
}

async fn post_exp_offer(
    State(s): State<AppState>,
    Json(batch): Json<ExperienceBatch>,
) -> StatusCode {
    if let Ok(mut exp) = s.experience.lock() {
        exp.push(batch);
        StatusCode::NO_CONTENT
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    }
}

async fn post_exp_buy(
    State(s): State<AppState>,
    Json(req): Json<BuyReq>,
) -> Result<Json<ExperienceBatch>, StatusCode> {
    let mut exp = s.experience.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let idx = exp
        .iter()
        .enumerate()
        .filter(|(_, b)| b.price.0 <= req.max_price)
        .min_by_key(|(_, b)| b.price.0)
        .map(|(i, _)| i);
    match idx {
        Some(i) => Ok(Json(exp.remove(i))),
        None => Err(StatusCode::NOT_FOUND),
    }
}

// ── Signal endpoints ────────────────────────────────────────────────────────

fn parse_mint(raw: &str) -> Result<Address, StatusCode> {
    Address::new(raw).map_err(|_| StatusCode::BAD_REQUEST)
}

async fn get_reputation(
    State(s): State<AppState>,
    Path(mint): Path<String>,
) -> Result<Json<scemadex_sdk::Reputation>, StatusCode> {
    let addr = parse_mint(&mint)?;
    s.signal
        .reputation(&addr)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn get_pool_score(
    State(s): State<AppState>,
    Path(mint): Path<String>,
) -> Result<Json<scemadex_sdk::PoolScore>, StatusCode> {
    let addr = parse_mint(&mint)?;
    s.signal
        .pool_score(&addr)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn get_advice(
    State(s): State<AppState>,
    Path(mint): Path<String>,
) -> Result<Json<scemadex_sdk::Advice>, StatusCode> {
    let addr = parse_mint(&mint)?;
    s.signal
        .advice(&addr)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
