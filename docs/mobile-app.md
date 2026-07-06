# Scematica Mobile App (`.apk`) — Build & Ship Runbook

A companion Android app that pairs to **your own** self-hosted sniper and gives it a
full remote: live metrics, pools, trades, PnL, NN status, logs **and** control (sell
mode, dump, rate mode, high-speed, moon-chase, builder mode). Distributed on the
**Solana dApp Store** and as a **direct-download `.apk`**.

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
.\gradlew.bat assembleRelease   # app-release.apk (dApp Store + direct download)
.\gradlew.bat bundleRelease     # app-release.aab (if you also list on Play)
```

`npm run mobile:apk` chains export → sync → `assembleRelease`.

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
Host `app-release.apk` on the site (`web/public/` or a release asset) with a QR + install
note ("enable install from unknown sources"). Zero gatekeeping, instant. No auto-update —
bump `versionCode` and re-host for updates.

### B. Solana dApp Store (the adoption channel)
Crypto-native, **no auto-trading/financial policy restriction** (unlike Google Play), and
it reaches exactly the Solana-trader audience.
1. `npm i -g @solana-mobile/dapp-store-cli`
2. Prepare `config.yaml` (name, icon, screenshots, the signed APK, an Android package
   `io.scematica.app`).
3. `dapp-store create publisher` / `create app` / `create release` (mints publisher +
   app + release NFTs on mainnet from a publisher keypair — needs a little SOL).
4. `dapp-store publish submit` → review.
   Full flow: <https://docs.solanamobile.com/dapp-publishing/intro>.

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

## Push notifications (wired — needs Firebase to activate)

Both halves ship:

- **Client** (`web/lib/push.ts`, called from `MobileGate` after pairing) — on native it
  requests permission, registers via `@capacitor/push-notifications`, and POSTs the FCM
  token to the paired instance (`/api/push/register`, token-gated). No-op on web.
- **Server** (`scematica-api`) — stores device tokens in `scematica-push-tokens.json`; a
  background task tails `scematica-trades.jsonl` and pushes on each new trade
  (`POST /api/push/test` sends a test). Both are **no-ops until `FCM_SERVER_KEY` is set**.

To activate:
1. Create a Firebase project, add an Android app (`io.scematica.app`), download
   `google-services.json` → `web/android/app/`.
2. Get the **Cloud Messaging server key** and set `FCM_SERVER_KEY` on the instance
   running `scematica-api`.
3. Restart the API; pair a device; `curl -H "Authorization: Bearer $TOKEN" -X POST
   <base>/api/push/test` to verify.

(Legacy FCM HTTP is used for simplicity — migrate to HTTP v1 / a service account when you
scale beyond a personal fleet.)

## Caveats

- **Static export limits.** The mobile build can't include the dynamic Next proxy
  routes; `scripts/mobile-export.mjs` relocates `app/api` during export and restores it.
  If you add server-only route handlers, they won't exist on mobile — route their data
  through `scematica-api` + `apiFetch` instead.
- **Cleartext/mixed content.** The app allows cleartext for LAN http instances
  (`capacitor.config.ts`). For anything off-LAN, pair over HTTPS/Tailscale.
- **MWA requires a wallet app** (Phantom/Solflare) installed on the device.
