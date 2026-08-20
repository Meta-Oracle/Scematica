//! The token, and the two attacks it is not enough to stop on its own.
//!
//! Binding to loopback keeps this off the network. It does **not** make the daemon private:
//! every other process on the machine can reach `127.0.0.1`, and so — via the user's own
//! browser — can every web page they visit. Three layers, each covering what the previous
//! one misses:
//!
//! 1. **Loopback bind** (`http::loopback`) — nothing off-machine can connect.
//! 2. **Bearer token** — a local process, or a page, must know a 256-bit secret it has no
//!    way to read. This is the layer that actually authorises.
//! 3. **Host header check** ([`host_is_local`]) — defeats DNS rebinding, where an attacker
//!    points `evil.example` at `127.0.0.1` so that a page's requests become *same-origin*
//!    with the daemon and the browser hands over the responses. The token would still be
//!    unknown to the page, but a rebinding attack is precisely how a same-origin page gets
//!    to read a response, so the cheap check goes in.
//!
//! Plus a fourth, by omission: the server emits no `Access-Control-Allow-Origin` and
//! handles no `OPTIONS`, so an ordinary cross-origin page cannot read a reply even if it
//! guesses a route. The browser extension is unaffected because it fetches from its service
//! worker under `host_permissions`, which is not subject to CORS.
//!
//! ## Comparison is constant-time
//!
//! `==` on a `String` returns as soon as two bytes differ, which leaks the length of the
//! matching prefix to anything that can time it. A local attacker can time it very
//! precisely.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Bytes of entropy in a token. 256 bits, hex-encoded to 64 characters.
pub const TOKEN_BYTES: usize = 32;

/// Where the token lives, relative to the state root.
pub const TOKEN_FILE: &str = "omnid.token";

/// Load the token for a state root, generating one on first run.
///
/// The file is the pairing mechanism: the operator reads it once and pastes it into the
/// extension. It is regenerated only if deleted, so a paired client stays paired across
/// restarts — a daemon that rotated its token on every start would be unusable.
pub fn load_or_create(root: &Path) -> Result<String> {
    let path = token_path(root);
    if let Ok(existing) = fs::read_to_string(&path) {
        let trimmed = existing.trim().to_string();
        if trimmed.len() >= 32 {
            return Ok(trimmed);
        }
        // A short or empty token file is a truncated write, not a policy choice. Replacing
        // it is safe; honouring it would install a weak secret nobody chose.
    }
    let token = generate()?;
    fs::create_dir_all(root).with_context(|| format!("creating {}", root.display()))?;
    let tmp = path.with_extension("token.tmp");
    fs::write(&tmp, &token).with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, &path).with_context(|| format!("renaming into {}", path.display()))?;
    restrict(&path);
    Ok(token)
}

pub fn token_path(root: &Path) -> PathBuf {
    root.join(TOKEN_FILE)
}

fn generate() -> Result<String> {
    let mut buf = [0u8; TOKEN_BYTES];
    getrandom::getrandom(&mut buf).context("reading OS entropy for the daemon token")?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

/// Best-effort permission tightening.
///
/// Unix only; on Windows the file inherits the user profile's ACL, which already excludes
/// other users. Deliberately not fatal on failure — a daemon that refuses to start because
/// it could not chmod is a daemon that does not start on a network share, and the token's
/// security does not rest on the mode bits.
#[cfg(unix)]
fn restrict(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict(_path: &Path) {}

/// Constant-time equality over the full length of both inputs.
///
/// Length is compared without early return too: an early `len` check leaks whether the
/// guess was the right size, which for a fixed-length token is uninteresting but for a
/// future variable-length one would not be.
pub fn secret_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = (a.len() ^ b.len()) as u8;
    let n = a.len().max(b.len());
    for i in 0..n {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        diff |= x ^ y;
    }
    diff == 0
}

/// Extract a bearer token from an `Authorization` header value.
///
/// Also accepts a bare token, because `curl -H "Authorization: <token>"` is what an
/// operator types first and refusing it teaches nothing.
pub fn bearer(header: &str) -> &str {
    let h = header.trim();
    match h.strip_prefix("Bearer ").or_else(|| h.strip_prefix("bearer ")) {
        Some(rest) => rest.trim(),
        None => h,
    }
}

/// Is the `Host` header one of this daemon's own names?
///
/// Anything else means the request arrived through a name that resolves here but is not
/// here — the shape of a DNS rebinding attack. An absent `Host` is rejected: HTTP/1.1
/// requires it, and the only clients that omit it are hand-written ones.
pub fn host_is_local(host: Option<&str>, port: u16) -> bool {
    let Some(host) = host else {
        return false;
    };
    let host = host.trim();
    // Strip the port if present; compare the name only.
    let name = match host.rsplit_once(':') {
        // An IPv6 literal is bracketed, so a colon inside brackets is not a port separator.
        Some((n, p)) if !n.ends_with(']') || p.chars().all(|c| c.is_ascii_digit()) => {
            if p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty() {
                let declared: u16 = p.parse().unwrap_or(0);
                if declared != port {
                    return false;
                }
            }
            n
        }
        _ => host,
    };
    matches!(name, "127.0.0.1" | "localhost" | "[::1]" | "::1")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "scema-omni-auth-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn a_token_is_generated_once_and_then_reused() {
        // A daemon that rotated its token on every restart would unpair the extension every
        // time the operator reboots.
        let dir = scratch();
        let a = load_or_create(&dir).unwrap();
        let b = load_or_create(&dir).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), TOKEN_BYTES * 2);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn two_daemons_do_not_share_a_token() {
        let (d1, d2) = (scratch(), scratch());
        assert_ne!(load_or_create(&d1).unwrap(), load_or_create(&d2).unwrap());
        fs::remove_dir_all(&d1).ok();
        fs::remove_dir_all(&d2).ok();
    }

    #[test]
    fn a_truncated_token_file_is_replaced_not_honoured() {
        let dir = scratch();
        fs::write(token_path(&dir), "abc").unwrap();
        let t = load_or_create(&dir).unwrap();
        assert_eq!(t.len(), TOKEN_BYTES * 2, "a short secret nobody chose must not be installed");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn secret_comparison_matches_only_the_exact_token() {
        assert!(secret_eq("abc", "abc"));
        assert!(!secret_eq("abc", "abd"));
        assert!(!secret_eq("abc", "abcd"), "a prefix must not authenticate");
        assert!(!secret_eq("abcd", "abc"));
        assert!(!secret_eq("", "a"));
        assert!(secret_eq("", ""));
    }

    #[test]
    fn bearer_accepts_both_the_prefixed_and_the_bare_form() {
        assert_eq!(bearer("Bearer deadbeef"), "deadbeef");
        assert_eq!(bearer("bearer deadbeef"), "deadbeef");
        assert_eq!(bearer("  deadbeef  "), "deadbeef");
    }

    #[test]
    fn a_rebinding_host_is_rejected() {
        // The attack: evil.example resolves to 127.0.0.1, so the page's requests become
        // same-origin with the daemon and the browser hands over the responses.
        assert!(!host_is_local(Some("evil.example:7842"), 7842));
        assert!(!host_is_local(Some("attacker.test"), 7842));
        assert!(!host_is_local(None, 7842), "HTTP/1.1 requires a Host header");
    }

    #[test]
    fn the_daemons_own_names_are_accepted() {
        assert!(host_is_local(Some("127.0.0.1:7842"), 7842));
        assert!(host_is_local(Some("localhost:7842"), 7842));
        assert!(host_is_local(Some("127.0.0.1"), 7842));
        assert!(host_is_local(Some("[::1]:7842"), 7842));
    }

    #[test]
    fn a_local_name_on_the_wrong_port_is_rejected() {
        // Another rebinding shape: a page on localhost:3000 addressing the daemon's port
        // through a Host that names its own.
        assert!(!host_is_local(Some("localhost:3000"), 7842));
    }
}
