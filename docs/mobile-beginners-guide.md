# Scematica Mobile App — Beginner's Guide
### Put your sniper in your pocket — no coding experience needed

The Scematica mobile app is a **remote control** for the bot you already run on your
computer. It does **not** trade on your phone and it never holds your wallet keys — it
just talks to *your own* running bot over your network and shows you what it's doing,
with buttons to steer it (pause buying, dump a position, change modes, hit the
kill‑switch). Your keys and your money stay on your computer, exactly where they are now.

If you can install an app and type a web address, you can do this.

---

## The big picture

```
   YOUR COMPUTER (runs the bot)                 YOUR PHONE (the remote)
   ┌───────────────────────────┐               ┌──────────────────────┐
   │  sniper  ── writes ──►  API │◄── network ──►│  Scematica app       │
   │  (keys, RPC, trading)  :3001│   + password  │  monitor + control   │
   └───────────────────────────┘               └──────────────────────┘
```

The phone connects to your computer, reads the bot's live state, and sends control
commands. Nothing sensitive leaves your computer.

---

## Before you start — what you need

- The **Scematica bot already set up** on a Windows computer. If you haven't done that
  yet, do the [Beginner's Guide](BEGINNER_GUIDE.md) first.
- An **Android phone** (Android 5.1 or newer).
- The phone and the computer on the **same Wi‑Fi network** (easiest). *(Off‑network
  access is possible later — see "Using it away from home" near the end.)*
- The app file **`scematica.apk`** (you built it, or download it from the release page).

---

## Step 1 — Put the app on your phone

You have two ways to install it.

**Option A — Install the file directly (works today)**
1. Get `scematica.apk` onto your phone. Easiest: email it to yourself, or put it in
   Google Drive, and open it on the phone. (On the computer it's at the project root as
   `scematica.apk`.)
2. Tap the file. Android will say *"For your security, your phone can't install unknown
   apps from this source."* Tap **Settings → allow from this source**, then go back and
   tap **Install**.
3. Open **Scematica**. You'll see a **pairing screen** — that's Step 4.

**Option B — Solana dApp Store** (once it's published)
Open the dApp Store on a Solana phone (Saga/Seeker), search **Scematica**, tap install.
No "unknown sources" step needed.

---

## Step 2 — Turn on the connection on your computer

The app talks to a small program called the **API** that comes with the bot. You need to
(1) pick a password and (2) start the API.

1. Open **PowerShell** in your Scematica project folder.
2. Pick a password (any long, random phrase — this is what stops strangers from
   controlling your bot) and start the API:

   ```powershell
   $env:SCEMATICA_API_TOKEN = "pick-a-long-random-password-here"
   cargo run --release --bin api
   ```

   Leave this window open. You should see `Scematica API listening on http://0.0.0.0:3001`.
3. Make sure the **bot itself is running too** (the dashboard or sniper), so there's live
   data to show. In another window:

   ```powershell
   cargo run --release --bin dashboard
   ```

> **Write your password down.** You'll type it into the phone once, in Step 4.

---

## Step 3 — Find your computer's address

The phone needs to know where your computer is on the network.

1. In a PowerShell window, run:

   ```powershell
   ipconfig
   ```
2. Look for **IPv4 Address** under your Wi‑Fi adapter — something like
   `192.168.1.50`. That's your computer's address.

Your bot's address for the phone is that number plus `:3001`, e.g.
**`http://192.168.1.50:3001`**.

---

## Step 4 — Pair the phone (connect the app to your bot)

On the phone, the app opens to the **pairing screen**. Fill it in:

| Field | What to type |
|---|---|
| **Instance URL** | `http://192.168.1.50:3001` (your address from Step 3) |
| **API token** | the password you chose in Step 2 |
| **Label** | anything, e.g. `home-pc` (optional) |

Tap **PAIR**. The app checks the connection and, if it works, drops you straight into the
live dashboard. 🎉

**Even easier — scan a QR code (optional).** If you run the web dashboard on your
computer, open **`http://localhost:3000/pair`** in a browser, type the same URL + token
there, and it draws a QR code. Point the app at it instead of typing. (The password stays
on your computer — it's only inside the QR you choose to show.)

---

## Step 5 — Using the app

Once paired, you get the same panels as the desktop dashboard, live on your phone:

- **Metrics & PnL** — profit/loss, win rate, open positions, wallet balance.
- **Pools & trades** — what the bot is finding and buying/selling right now.
- **NN status** — the Deep Q* agent's learning state.
- **Controls** — the buttons that actually steer the bot:
  - **Sell mode** — stop buying, sell what's open.
  - **Dump mode** — force‑sell everything immediately (the kill‑switch).
  - **Rate mode** — how aggressive to be (bearish → moon).
  - **High‑speed**, **Moon‑chase**, **Builder mode** — strategy toggles.

Tapping a control sends the command to your computer instantly; the bot picks it up on
its next check.

---

## Step 6 — Turn on notifications (optional, advanced — not in the default app)

The standard app **does not** include push notifications — on purpose. (Notifications need
Google Firebase, and bundling them without a Firebase account makes the app crash on
launch, so we leave them out to keep the app rock‑solid.)

If you want a phone alert on every new trade — even with the app closed — it's a developer
step that requires your own free Firebase project and **rebuilding the app**. The full
recipe is in [mobile-app.md → "Push notifications"](mobile-app.md). In short: create a
Firebase project + a service account, add `google-services.json`, re‑install the
`@capacitor/push-notifications` plugin, restore the push code, set `FCM_SERVICE_ACCOUNT`
on the API, and rebuild. Most people can happily skip this — just keep the app open when
you want to watch.

---

## Staying safe

- **Always set a password** (`SCEMATICA_API_TOKEN`) before pairing. Without it, anyone on
  your network could steer your bot.
- **Don't put your computer's `:3001` port directly on the public internet.** On your home
  Wi‑Fi it's fine. To use it away from home, use the encrypted option below.
- The app only ever sends the password back to *your* computer. It's never shared.

---

## Using it away from home (optional)

To control the bot when you're not on the same Wi‑Fi, use **Tailscale** — a free app that
privately connects your phone and computer over the internet, encrypted:

1. Install Tailscale on both the computer and the phone, sign into the same account.
2. On the computer, Tailscale gives it a name/address (e.g. `100.x.x.x` or a
   `something.ts.net` name).
3. Pair the app to **`http://<that-tailscale-address>:3001`** with your token.

Now it works from anywhere, safely, with no ports exposed to strangers.

---

## Troubleshooting

**"Could not reach the instance, or the token was rejected."**
- Is the API window still open and showing *"listening on … :3001"*? (Step 2)
- Are the phone and computer on the **same Wi‑Fi**? (Guest networks often block this.)
- Did you type the address exactly, starting with `http://` and ending in `:3001`?
- Does the password on the phone match `SCEMATICA_API_TOKEN` **exactly** (no extra spaces)?
- Try opening `http://<computer-ip>:3001/health` in the phone's browser — it should say
  `ok`. If it doesn't, it's a network/firewall issue, not the app.

**Windows blocks the connection.** The first time you run the API, Windows Firewall may
pop up — click **Allow access** (Private networks). If you missed it, allow `cargo`/the
API through Windows Firewall for private networks.

**The app opens but panels are empty.** The API is reachable but the **bot isn't running**
(no data to show). Start the dashboard/sniper (Step 2, item 3).

**Controls don't do anything.** Controls only work when a password is set on the API and
the same password is paired on the phone. Re‑check Step 2 and Step 4.

**I need to re‑pair / point at a different computer.** Reinstalling the app resets pairing;
or clear the app's storage in Android **Settings → Apps → Scematica → Storage → Clear**.

---

## FAQ

**Does the app trade on my phone?** No. It's a remote. All trading happens on your
computer, using the keys that never leave it.

**Do I need my wallet on the phone?** No. The token gate uses a normal wallet app
(Phantom) only for the one‑time membership check; the bot's trading key stays on the
computer.

**Is my money at risk from the phone?** The phone can *steer* the bot (including the
kill‑switch), so treat the password like a key. It cannot move funds on its own.

**Will it drain my battery?** No — it only does work while you have it open looking at it.

---

*Developer/build details (signing, publishing to the dApp Store, the API endpoints) live
in [mobile-app.md](mobile-app.md). This guide is just for getting the app running.*
