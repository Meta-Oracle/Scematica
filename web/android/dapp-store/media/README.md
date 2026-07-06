# dApp Store media assets

Drop the store assets referenced by `../config.yaml` here (git-ignored — they're binaries):

- `icon.png` — 512×512 (or larger) app icon, PNG.
- `screenshot-1.png`, `screenshot-2.png`, … — phone screenshots of the app (pairing
  screen + the live dashboard/controls make good ones). Portrait, real device or emulator.

Capacitor generated a default launcher icon at `../app/src/main/res/mipmap-*/`; replace it
with the real Scematica mark before shipping (e.g. `npx @capacitor/assets generate` from a
1024×1024 source in `web/assets/`).
