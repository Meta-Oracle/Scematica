//! Serves sealed world trees to the holders of the tokens that commit to them.
//!
//! ## Why this is a separate binary and not a flag on `scema-omnid`
//!
//! The daemon binds loopback and that bind is **deliberately not configurable** — the note in
//! `scema-daemon` says the one thing that reliably happens to a `--bind` flag is somebody
//! setting it to `0.0.0.0`. It emits no `Access-Control-Allow-Origin` and handles no
//! `OPTIONS`, so a web page cannot read its replies even if it guesses a route.
//!
//! A token-gated distribution service is the opposite of all of that: it is *supposed* to be
//! reachable. Adding a `--public` flag to the daemon would put both postures in one process
//! and one config file, and the failure mode is somebody enabling the wrong one. So this is a
//! different binary with a different default, and `scema-omnid` keeps its guarantee intact.
//!
//! ## What it will and will not serve
//!
//! It serves **sealed decision records**, to an address that holds the token committing to
//! that record's world. One token, one world — see `scema-entitlement`.
//!
//! It has **no write path at all**. No `decide`, no `execute`, no `observe`. Reading a corpus
//! and acting on a workspace are different powers and this process only has the first; there
//! is no flag that grants the second, which is stronger than a flag that defaults to off.
//!
//! ## The guarantee this must never damage
//!
//! A record somebody already holds verifies with **no server, no key and no permission** —
//! `scema verify --file` and `/omni` both do it offline. This gates *distribution*, never
//! *truth*. A holder who fetches a record once owes this service nothing afterwards, and if
//! that ever stops being true something has gone badly wrong with the design.
//!
//! ## TLS
//!
//! There is none, on purpose. Put this behind a reverse proxy that terminates TLS. The
//! alternative is a TLS stack in the omni workspace, which is the dependency the whole
//! workspace split exists to avoid.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use scema_daemon::http::{Request, Response};
use scema_entitlement::{
    authorise, is_digest, Decision, Entitlement, Holder, OwnershipOracle, TokenRef,
};

pub mod store;

pub use store::RecordStore;

/// Everything a request needs.
pub struct Vault {
    store: RecordStore,
    oracle: Box<dyn OwnershipOracle + Send + Sync>,
    /// Which token contract entitles which world. Loaded once, from a file the operator
    /// writes — this process never mints and never guesses.
    entitlements: BTreeMap<String, Entitlement>,
}

impl Vault {
    pub fn new(
        root: impl AsRef<Path>,
        oracle: Box<dyn OwnershipOracle + Send + Sync>,
        entitlements: Vec<Entitlement>,
    ) -> Self {
        Vault {
            store: RecordStore::new(root),
            oracle,
            entitlements: entitlements
                .into_iter()
                .map(|e| (e.world_commitment.clone(), e))
                .collect(),
        }
    }

    pub fn entitlement_count(&self) -> usize {
        self.entitlements.len()
    }

    /// Route a request. The whole surface is three GETs.
    pub fn handle(&self, req: Request) -> Response {
        match (req.method.as_str(), req.path.as_str()) {
            ("GET", "/health") => json(200, &serde_json::json!({
                "service": "scema-vault",
                "entitlements": self.entitlements.len(),
                // Deliberately not the record count. That would let anyone size the corpus
                // without holding anything, and the count is business information.
            })),
            ("GET", "/worlds") => self.catalogue(),
            ("GET", p) if p.starts_with("/world/") => {
                self.world(p.trim_start_matches("/world/"), &req)
            }
            ("GET", _) => json(404, &serde_json::json!({ "error": "no such route" })),
            _ => json(405, &serde_json::json!({ "error": "this service is read-only" })),
        }
    }

    /// What worlds exist, by commitment. **Public on purpose.**
    ///
    /// A buyer has to be able to see what a token would entitle them to *before* buying it,
    /// and a digest is not the world — it is the thing the world hashes to. Publishing the
    /// list is what makes the market legible; publishing the records is what the gate is for.
    fn catalogue(&self) -> Response {
        let items: Vec<_> = self
            .entitlements
            .values()
            .map(|e| {
                serde_json::json!({
                    "world_commitment": e.world_commitment,
                    "token": { "chain": e.token.chain, "contract": e.token.contract,
                               "token_id": e.token.token_id },
                    "held": self.store.has(&e.world_commitment),
                })
            })
            .collect();
        json(200, &serde_json::json!({ "worlds": items }))
    }

    fn world(&self, commitment: &str, req: &Request) -> Response {
        if !is_digest(commitment) {
            return json(400, &serde_json::json!({
                "error": "not a commitment",
                "detail": "a world is addressed by 64 lowercase hex characters",
            }));
        }
        let Some(holder) = req.header("x-scema-holder").map(|h| Holder(h.to_string())) else {
            return json(401, &serde_json::json!({
                "error": "no holder",
                "detail": "send X-Scema-Holder with the address that holds the token",
            }));
        };
        let Some(ent) = self.entitlements.get(commitment) else {
            // 404, not 403. Refusing to say whether an unlisted world exists would be
            // security theatre here — `/worlds` publishes the catalogue.
            return json(404, &serde_json::json!({
                "error": "no such world",
                "detail": "nothing in this vault commits to that digest",
            }));
        };

        match authorise(self.oracle.as_ref(), ent, &holder, commitment) {
            Decision::Granted { .. } => match self.store.read(commitment) {
                Some(text) => Response {
                    status: 200,
                    content_type: "application/json; charset=utf-8".into(),
                    body: text.into_bytes(),
                    extra: vec![(
                        // Said in the response, not only in a README. A holder who fetches
                        // this owes the service nothing afterwards.
                        "X-Scema-Verify".into(),
                        "offline: scema verify --file, or the /omni page".into(),
                    )],
                },
                None => json(404, &serde_json::json!({
                    "error": "entitled, but not stored",
                    "detail": "the token commits to a world this vault does not have — that is \
                               a gap in the vault, not in your entitlement",
                })),
            },
            // 403 for a fact about the holder.
            Decision::Denied { reason } => json(403, &serde_json::json!({
                "error": "denied",
                "detail": reason.explain(),
            })),
            // 503, never 403. "You do not own this" and "the chain would not answer" are
            // different facts and only one is about the holder — a reader told the first
            // goes and buys a token they already have.
            Decision::Undetermined { why } => json(503, &serde_json::json!({
                "error": "undetermined",
                "detail": format!("{why} — this is not a denial; retry"),
                "retry": true,
            })),
        }
    }
}

fn json(status: u16, v: &serde_json::Value) -> Response {
    Response::json(status, serde_json::to_vec(v).unwrap_or_default())
}

/// Read an entitlement manifest: a JSON array of `{ token, world_commitment }`.
pub fn load_entitlements(path: &PathBuf) -> anyhow::Result<Vec<Entitlement>> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
    let raw: Vec<serde_json::Value> = serde_json::from_str(&text)?;
    let mut out = Vec::new();
    for (i, v) in raw.iter().enumerate() {
        let token = TokenRef {
            chain: field(v, i, "chain")?,
            contract: field(v, i, "contract")?,
            token_id: field(v, i, "token_id")?,
        };
        let commitment = field(v, i, "world_commitment")?;
        if !is_digest(&commitment) {
            anyhow::bail!("entry {i}: `{commitment}` is not 64 lowercase hex characters");
        }
        out.push(Entitlement { token, world_commitment: commitment });
    }
    Ok(out)
}

fn field(v: &serde_json::Value, i: usize, name: &str) -> anyhow::Result<String> {
    v.get(name)
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("entry {i}: missing `{name}`"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use scema_entitlement::Ownership;

    struct Fixed(Ownership);
    impl OwnershipOracle for Fixed {
        fn holds(&self, _t: &TokenRef, _h: &Holder) -> Ownership {
            self.0.clone()
        }
    }

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn scratch() -> PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "scema-vault-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn vault(ownership: Ownership, store_it: bool) -> (Vault, PathBuf) {
        let root = scratch();
        if store_it {
            std::fs::write(root.join(format!("{A}.json")), r#"{"id":"x"}"#).unwrap();
        }
        let e = Entitlement {
            token: TokenRef { chain: "c".into(), contract: "k".into(), token_id: "1".into() },
            world_commitment: A.into(),
        };
        (Vault::new(&root, Box::new(Fixed(ownership)), vec![e]), root)
    }

    fn get(path: &str, holder: Option<&str>) -> Request {
        let mut headers = BTreeMap::new();
        if let Some(h) = holder {
            headers.insert("x-scema-holder".into(), h.to_string());
        }
        Request {
            method: "GET".into(),
            path: path.into(),
            query: BTreeMap::new(),
            headers,
            body: vec![],
        }
    }

    #[test]
    fn a_holder_gets_the_record() {
        let (v, _r) = vault(Ownership::Held, true);
        let res = v.handle(get(&format!("/world/{A}"), Some("0x1")));
        assert_eq!(res.status, 200);
        assert!(res.extra.iter().any(|(k, _)| k == "X-Scema-Verify"),
            "the response must say the record verifies offline");
    }

    #[test]
    fn a_non_holder_gets_403() {
        let (v, _r) = vault(Ownership::NotHeld, true);
        assert_eq!(v.handle(get(&format!("/world/{A}"), Some("0x2"))).status, 403);
    }

    #[test]
    fn an_unreadable_chain_is_503_and_never_403() {
        // The rule that survives all the way to the wire. A 403 tells a holder they do not
        // own something they may well own, and they go and buy it again.
        let (v, _r) = vault(Ownership::Unknown { why: "rpc down".into() }, true);
        let res = v.handle(get(&format!("/world/{A}"), Some("0x1")));
        assert_eq!(res.status, 503);
        let body: serde_json::Value = serde_json::from_slice(&res.body).unwrap();
        assert_eq!(body["retry"], true);
        assert!(body["detail"].as_str().unwrap().contains("not a denial"));
    }

    #[test]
    fn a_request_without_a_holder_is_401_before_the_chain_is_touched() {
        struct Exploding;
        impl OwnershipOracle for Exploding {
            fn holds(&self, _t: &TokenRef, _h: &Holder) -> Ownership {
                panic!("ownership must not be consulted without a holder");
            }
        }
        let root = scratch();
        let e = Entitlement {
            token: TokenRef { chain: "c".into(), contract: "k".into(), token_id: "1".into() },
            world_commitment: A.into(),
        };
        let v = Vault::new(&root, Box::new(Exploding), vec![e]);
        assert_eq!(v.handle(get(&format!("/world/{A}"), None)).status, 401);
    }

    #[test]
    fn entitled_but_missing_says_the_gap_is_the_vaults() {
        // A holder refused because of the operator's incomplete corpus must not be told
        // anything that sounds like a problem with their token.
        let (v, _r) = vault(Ownership::Held, false);
        let res = v.handle(get(&format!("/world/{A}"), Some("0x1")));
        assert_eq!(res.status, 404);
        let body: serde_json::Value = serde_json::from_slice(&res.body).unwrap();
        assert!(body["detail"].as_str().unwrap().contains("gap in the vault"));
    }

    #[test]
    fn there_is_no_write_path_at_all() {
        // Not a flag defaulting to off — no route. Reading a corpus and acting on a
        // workspace are different powers and this process only ever has the first.
        let (v, _r) = vault(Ownership::Held, true);
        for method in ["POST", "PUT", "DELETE", "PATCH"] {
            let mut req = get("/world", Some("0x1"));
            req.method = method.into();
            assert_eq!(v.handle(req).status, 405, "{method} was not refused");
        }
    }

    #[test]
    fn the_catalogue_is_public_so_a_buyer_can_see_what_a_token_would_buy() {
        // A digest is not the world; it is what the world hashes to. Publishing the list is
        // what makes the market legible, and the gate is on the records.
        let (v, _r) = vault(Ownership::NotHeld, true);
        let res = v.handle(get("/worlds", None));
        assert_eq!(res.status, 200);
        let body: serde_json::Value = serde_json::from_slice(&res.body).unwrap();
        assert_eq!(body["worlds"][0]["world_commitment"], A);
    }

    #[test]
    fn health_does_not_leak_the_corpus_size() {
        let (v, _r) = vault(Ownership::Held, true);
        let body: serde_json::Value =
            serde_json::from_slice(&v.handle(get("/health", None)).body).unwrap();
        assert!(body.get("records").is_none(), "record count is business information");
    }

    #[test]
    fn a_malformed_commitment_is_rejected_before_anything_else() {
        let (v, _r) = vault(Ownership::Held, true);
        assert_eq!(v.handle(get("/world/../../etc/passwd", Some("0x1"))).status, 400);
        assert_eq!(v.handle(get(&format!("/world/{}", A.to_uppercase()), Some("0x1"))).status, 400);
    }
}
