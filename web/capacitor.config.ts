import type { CapacitorConfig } from '@capacitor/cli'

// Scematica mobile shell. Bundles the static export in `out/` and talks to the
// operator's own paired sniper API (lib/net.ts). Cleartext is allowed so a self-hosted
// instance reachable over plain http on a LAN (http://192.168.x.x:3001) works without
// a reverse proxy — pair over Tailscale/HTTPS for anything off-LAN.
const config: CapacitorConfig = {
  appId: 'io.scematica.app',
  appName: 'Scematica',
  webDir: 'out',
  android: {
    allowMixedContent: true,
  },
  server: {
    androidScheme: 'http',
    cleartext: true,
  },
}

export default config
