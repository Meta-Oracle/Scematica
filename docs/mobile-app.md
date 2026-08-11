# Scematica Mobile App (`.apk`) — Build & Ship Runbook

A companion Android app that pairs to **your own** self-hosted sniper and gives it a
full remote: live metrics, pools, trades, PnL, NN status, logs **and** control (sell
mode, dump, rate mode, high-speed, moon-chase, builder mode). Distributed on the
**Solana dApp Store** and as a **direct-download `.apk`**.

> **Just want to run the app, not build it?** See the plain-English
> [Mobile Beginner's Guide](mobile-beginners-guide.md) — install, pair, and use, with no
> coding. This page is the developer build/ship runbook.

## The one hard rule

The sniper does **not** run inside the `.apk`. It is `solana-sdk` + a persistent Helius
WebSocket + child-process spawning + latency-sensitive execution — it belongs on a VPS
or PC near your RPC. The app is a **thin remote**: it wraps the existing `web/` dashboard
with [Capacitor](https://capacitorjs.com) and talks to your sniper's `scematica-api`
over HTTP. Your keys and capital never leave your machine; the phone only sends
signed-in-advance control commands over a token-gated API.

```
┌── your box (VPS / PC) ───────────────┐         ┌── your phone ─────────┐
│ sniper  ──file IPC──►  scematica-api │◄──HTTP──►│ Scematica.apk (remote)│
│ (keys, RPC, WebSocket)   :3001       │  +token  │ pairs to your API URL │
└──────────────────────────────────────┘         └───────────────────────┘
```

## Architecture already in place

- `crates/scematica-api` serves all read endpoints (`/api/metrics|pools|trades|nn|…`)
  **and** the control POSTs (`/api/controls/{sell-mode,dump-mode,rate-mode,high-speed,
  moon-chase,builder-mode}`). CORS is open (`Any`); control routes are **token-gated**
  by `require_token` — set `SCEMATICA_API_TOKEN` and they require `Authorization: Bearer <token>`.
- `web/lib/net.ts` — `apiFetch` targets the paired instance and injects the token on
  native; on web it's a no-op passthrough to the same-origin Next proxy.
- `web/components/Pairing.tsx` + `MobileGate.tsx` — the first-run pairing screen (URL +
  token, or a `scematica://pair?url=…&token=…` string / QR).
- `web/capacitor.config.ts`, `web/scripts/mobile-export.mjs`, `MOBILE_EXPORT` in
  `next.config.js` — the static-export + Capacitor wiring.

## Prerequisites

| Tool | Version | Notes |
|---|---|---|
| Node | ✓ 22.19 present | |
| JDK | 17 | **not on PATH here.** Use Android Studio's bundled JBR or install Temurin 17, then `JAVA_HOME`. |
| Android Studio + SDK | latest | `ANDROID_HOME` is already set (`…\AppData\Local\Android\Sdk`). Install SDK Platform 34 + Build-Tools. |
| Capacitor CLI | 6.x | installed via `npm install` (added to `web/package.json`). |

## One-time backend setup (per operator)

1. **Set a token** and start the API next to the sniper:
   ```powershell
   $env:SCEMATICA_API_TOKEN = [Convert]::ToBase64String((1..24 | % { Get-Random -Max 256 }))
   echo $env:SCEMATICA_API_TOKEN     # you'll pair the phone with this
   cargo run --release --bin api      # listens on 0.0.0.0:3001
   ```
2. **Expose the API to the phone.** Pick one:
   - **LAN** — same Wi-Fi, pair to `http://<box-lan-ip>:3001` (cleartext is allowed by
     the app for LAN). Easiest for a home box.
   - **Tailscale** — install on both; pair to the box's MagicDNS name. Works anywhere,
     encrypted, no port-forwarding. **Recommended.**
   - **Reverse proxy (Caddy/nginx)** — put TLS in front, pair to `https://…`. Required
     if you ever expose it to the public internet.

   Never port-forward `:3001` to the open internet without TLS **and** the token.

## Build the `.apk`

```powershell
cd web
npm install                     # pulls Capacitor + Solana Mobile Wallet Adapter
npm run mobile:export           # static export → web/out  (relocates app/api, restores it)
npx cap add android             # one-time: scaffolds web/android (Gradle project)
npm run mobile:sync             # re-export + copy assets into the Android project
```

Then produce the binary:

```powershell
# Debug APK (sideload/testing), no signing needed:
cd android; .\gradlew.bat assembleDebug
#   → android/app/build/outputs/apk/debug/app-debug.apk

# Release APK/AAB (see signing below):
.\gradlew.bat assembleRelease   # → scematica-v<version>.apk (dApp Store + direct download)
.\gradlew.bat bundleRelease     # → .aab (if you also list on Play)
```

**Versioning (single source):** the app's `versionName`/`versionCode` and the artifact
name are derived from `web/package.json`'s `version` at build time (read in
`app/build.gradle`) — `versionCode` is the semver packed as `major*10000+minor*100+patch`
(1.25.0 → 12500), and the release artifact is named **`scematica-v<version>.apk`**. Bump
`web/package.json` to version the whole app; nothing else to edit.

`npm run mobile:apk` chains export → sync → `assembleRelease`. The release output lands at
`web/android/app/build/outputs/apk/release/scematica-v<version>.apk` (e.g.
`scematica-v1.25.0.apk`).

> **The checked-in apk is well behind the current source.** `web/package.json` is at
> 1.25.0; the last artifact built and copied to the project root is still
> `scematica-v1.11.3.apk`. Re-run `npm run mobile:apk` to produce a 1.25.0 build — it is
> needed anyway, because the static export must be rebuilt for the
> `NEXT_PUBLIC_STATIC_EXPORT` flag (see the pairing section) to reach the bundle.

> A signed `scematica-v1.11.3.apk` (v1+v2 schemes, verified) has already been built and
> copied to the **project root** for convenience. The release keystore is
> `web/android/scematica-release.jks` with creds in `web/android/keystore.properties`
> (both git-ignored) — **back up the keystore; losing it means you can't ship updates.**
>
> ⚠️ **Do not put the built `.apk` in `web/public/`.** Next copies `public/` into the
> export and cap sync bundles it into the *next* apk — the apk nests inside itself and
> compounds ~15 MB per build. `scripts/mobile-export.mjs` deletes any `public/*.apk`
> before building as a guard. Distribute via a GitHub release, not `public/`.

## Signing (release)

```powershell
# 1. Generate a keystore ONCE and keep it safe (losing it = can't update the app):
keytool -genkey -v -keystore scematica-release.jks -keyalg RSA -keysize 2048 `
        -validity 10000 -alias scematica

# 2. android/keystore.properties  (git-ignored):
#    storeFile=../../scematica-release.jks
#    storePassword=…
#    keyAlias=scematica
#    keyPassword=…
```

`npm run mobile:signing` automates step 3+: it writes an `android/keystore.properties`
template (fill it in), injects a `signingConfigs.release` block that reads it into
`android/app/build.gradle`, and points `buildTypes.release` at it. Idempotent — safe to
re-run. `keystore.properties` and `*.jks` are git-ignored.

## Wallet connect on device (token gate)

Native uses the **Phantom deeplink protocol** (`lib/mobileWallet.ts` + `MobileWalletContext`)
— **not** Mobile Wallet Adapter, whose web bridge (`@solana-mobile/wallet-standard-mobile`)
*refuses to run in a WebView* (it detects the `wv` user-agent and errors). The deeplink flow
is the WebView-compatible path:

1. `WalletStatus` shows a native **Connect Wallet** picker (Phantom / Solflare / Backpack).
2. Connecting generates an ephemeral x25519 keypair and opens the wallet app over its
   universal link (`https://phantom.app/ul/v1/connect?...`) with `redirect_link=scematica://wallet`.
3. The wallet returns to `scematica://wallet?...` (an intent-filter in `AndroidManifest.xml`
   reopens the app); `@capacitor/app`'s `appUrlOpen` fires, and we decrypt the payload
   (`nacl.box`) to get the connected address + session.
4. `useActiveWallet()` unifies this with the browser wallet-adapter, so the SCEMA gate and
   `WalletStatus` consume one identity on both platforms.

Phantom and Solflare speak this protocol identically; Backpack is best-effort. The gate is
read-only (address → SCEMA balance), so connect is all it needs; a signing path
(`signMessage`/`signTransaction` over the same encrypted channel + session) can be layered
on later. Requires the wallet app installed; the app never holds a key. On web, the
browser-extension wallet-adapter flow is unchanged.

## Distribution

### A. Direct `.apk` download (ship today)
`scematica-v<version>.apk` (e.g. `scematica-v1.11.3.apk`) is at the **project root**. Host
it as a **GitHub Release asset** (the canonical, versioned home) and link/QR to it. **Do
not** serve it from `web/public/` — that nests the apk inside itself (see the warning
above). Add an "enable install from unknown sources" note. No auto-update — bump
`web/package.json` and re-host for each update.

### B. Solana dApp Store (the adoption channel)
Crypto-native, **no auto-trading/financial policy restriction** (unlike Google Play), and
it reaches exactly the Solana-trader audience. A starter config is scaffolded at
`web/android/dapp-store/config.yaml` (app metadata + descriptions pre-filled; `address`
fields are filled by the CLI; drop store assets in `dapp-store/media/`).
1. `npm i -g @solana-mobile/dapp-store-cli`
2. `npx dapp-store init` to refresh the schema, then merge the scaffold's values.
3. `dapp-store create publisher -k <publisher-keypair> -u <rpc>` then `create app` then
   `create release` (mints publisher/app/release NFTs on mainnet from a publisher keypair
   — needs a little SOL; writes the `address` fields back into `config.yaml`).
4. `dapp-store publish submit -k <keypair> -u <rpc> --requestor-is-authorized ...` → review.
   Full flow: <https://docs.solanamobile.com/dapp-publishing/intro>.

**What still needs you:** a funded publisher keypair (a little mainnet SOL), a 512×512
app icon + phone screenshots in `dapp-store/media/`, and running the four commands above.
Everything else — signed APK, package id, metadata — is done.

### C. Google Play (optional, monitor-only)
Play's financial-services/crypto policy will likely reject an auto-trading remote. If you
want Play reach, submit a **monitor-only** variant (hide the control panel via a build
flag) as `app-release.aab`.

## Pairing UX

The app opens to the pairing screen (`MobileGate`). The frictionless path: visit
**`/pair` on your web dashboard** (`app/pair/page.tsx`) — enter the API base URL + token
and it renders a `scematica://pair?url=…&token=…&label=…` **QR** (the token stays in your
browser, only encoded into the QR). The app parses that string (paste today; in-app QR
scan is a small follow-up with `@capacitor/barcode-scanner`). Pairing is stored in
`localStorage` and every panel re-fetches against the paired instance.

**The desktop web dashboard can pair too** (`components/OfflineBanner.tsx`) — this is
what makes a *publicly hosted* `web/` deploy standalone: it doesn't need `scematica-api`
running on the same box as the Next.js server. When no bot is reachable (no pairing yet,
or a paired instance went offline), the banner offers a "Pair with your instance ↗" action
that opens the same `Pairing` component used by the app, storing the pairing in
`localStorage` exactly as on mobile. Everything that *doesn't* need a bot at all — the CA
banner, price ticker, buy links, wallet connect + SCEMA gate check — already talks
directly to public Solana RPC / Jupiter / DexScreener and needs no pairing or API.

## Standalone web: the self-contained API

`web/` runs with **no backend of any kind**. `app/api/[...slug]/route.ts` resolves each
request in two tiers:

1. **Live** — if `RUST_API_URL` points at a reachable `scematica-api`, proxy it (real
   trades, real money, control routes work). Reachability is cached for 15s so a deploy
   with no bot doesn't pay the connect timeout on every poll.
2. **Simulation** — otherwise, serve `lib/sim/engine.ts`, which runs entirely inside the
   Next.js server. No database, no external service, no Rust process.

The simulation is **honest about what it is**. Responses carry `simulated: true` and an
`X-Scematica-Source: simulation` header; `useDataSource()` turns that into a
non-dismissible amber SIMULATION banner plus a SIMULATION chip in the header, and control
POSTs return **503** rather than pretending a toggle applied. Simulated PnL must never be
mistaken for real trading results.

**What is genuinely real in simulation mode:** the Deep Q*™ network itself.
`lib/sim/dqstar.ts` is a full Dueling Double-DQN in TypeScript mirroring
`crates/scematica-nn` — `STATE_DIM(24) → 128 → 64 → {V, A}`, He init, ReLU,
`Q = V + A − mean(A)`, online/target pair, replay buffer, ε-greedy, and real
backpropagation. Forward passes and gradient steps actually execute per request; average
loss visibly converges (~0.42 → ~0.21 over a session) because the net is really training.
Only the market feeding it is synthetic.

Determinism matters on serverless: the session is a pure function of
`(SEED, elapsed-time-in-cycle)`, so a cold lambda reproduces exactly what a warm one had.
A session runs 6h then reseeds. A full replay costs ~0.5s and is cached for 4s.

## Push notifications (opt-in — NOT in the default build)

> ⚠️ **Why it's opt-in:** `@capacitor/push-notifications` pulls in Firebase Messaging,
> whose `FirebaseInitProvider` runs at process startup and requires a valid
> `google-services.json` / `google_app_id`. Bundling it **without** that config makes the
> app **crash instantly on launch**. So the default build ships **without** the plugin
> (crash-free); `web/lib/push.ts` is a no-op stub. Turn push on deliberately, per operator.

The **server half already ships** and is inert until configured: `scematica-api` exposes
`POST /api/push/register` + `/api/push/test` (token-gated), stores device tokens in
`scematica-push-tokens.json`, and a background task tails `scematica-trades.jsonl` to push
on each new trade — all **no-ops until `FCM_SERVICE_ACCOUNT` (or legacy `FCM_SERVER_KEY`)
is set**. It sends via **FCM HTTP v1** (RS256 JWT → cached OAuth2 token →
`/v1/projects/<id>/messages:send`).

To enable push end-to-end:
1. `cd web && npm i @capacitor/push-notifications`.
2. Create a Firebase project, add an Android app (`io.scematica.app`), download
   `google-services.json` → `web/android/app/` (git-ignored). The Capacitor android
   project already conditionally applies `com.google.gms.google-services` when that file
   is present, so no Gradle edits are needed.
3. Restore the real `initPush()` body in `web/lib/push.ts` (the implementation is kept in
   the file's header comment).
4. Create a **service account** (Firebase console → Project settings → Service accounts →
   *Generate new private key*), put the JSON on the box, and point the API at it:
   `FCM_SERVICE_ACCOUNT=C:\path\to\service-account.json`; restart `scematica-api`.
5. `npm run mobile:apk`, reinstall, pair a device, then verify:
   `curl -H "Authorization: Bearer $TOKEN" -X POST <base>/api/push/test`.

## Caveats

- **Static export limits.** The mobile build can't include the dynamic Next proxy
  routes; `scripts/mobile-export.mjs` relocates `app/api` during export and restores it.
  If you add server-only route handlers, they won't exist on mobile — route their data
  through `scematica-api` + `apiFetch` instead.
- **Cleartext/mixed content.** The app allows cleartext for LAN http instances
  (`capacitor.config.ts`). For anything off-LAN, pair over HTTPS/Tailscale.
- **MWA requires a wallet app** (Phantom/Solflare) installed on the device.
