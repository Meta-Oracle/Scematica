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
.\gradlew.bat assembleRelease   # → scematica.apk (dApp Store + direct download)
.\gradlew.bat bundleRelease     # → .aab (if you also list on Play)
```

`npm run mobile:apk` chains export → sync → `assembleRelease`. The release output is
named **`scematica.apk`** (via the `applicationVariants` rename in `app/build.gradle`) and
lands at `web/android/app/build/outputs/apk/release/scematica.apk`.

> A signed `scematica.apk` (v1+v2 schemes, verified) has already been built in this repo
> and copied to the project root and `web/public/scematica.apk`. The release keystore is
> `web/android/scematica-release.jks` with creds in `web/android/keystore.properties`
> (both git-ignored) — **back up the keystore; losing it means you can't ship updates.**

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

## Wallet signing on device (token gate + on-chain actions)

The token gate works out of the box: the existing web `PhantomWalletAdapter` deep-links
to the Phantom app from the WebView for the 250k-$SCEMA gate signature — the app never
holds a key. For a fully native signing UX later, add **Solana Mobile Wallet Adapter**
via `@solana-mobile/wallet-standard-mobile` (the *wallet-standard* build, which does not
drag in React Native — do **not** use `@solana-mobile/wallet-adapter-mobile` 2.2+, it
peer-requires React 19 and breaks the React 18 tree). Register it alongside the Phantom
adapter so on Android it routes through MWA.

## Distribution

### A. Direct `.apk` download (ship today)
`scematica.apk` is already at the project root and `web/public/scematica.apk` (served at
`/scematica.apk`). Because it's git-ignored, host it as a **GitHub Release asset** (the
canonical, versioned home) and/or un-ignore `web/public/scematica.apk` to serve it from
the dashboard. Add a QR + "enable install from unknown sources" note. No auto-update —
bump `versionCode` in `app/build.gradle` and re-host for each update.

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
