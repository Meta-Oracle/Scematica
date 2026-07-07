// Mobile wallet connect via the Phantom deeplink protocol (Phantom + Solflare speak it
// identically; Backpack is best-effort). This is the WebView-compatible way to connect a
// real wallet: the app opens the wallet app over a universal link, the user approves, and
// the wallet returns to `scematica://wallet` with an encrypted payload we decrypt to get
// the connected address. Read-only connect satisfies the SCEMA token gate; a signing path
// (signMessage/signTransaction over the same encrypted channel) can be layered on later.
//
// Protocol: https://docs.phantom.com/phantom-deeplinks/provider-methods/connect
import nacl from 'tweetnacl'
import bs58 from 'bs58'

export type WalletProvider = 'phantom' | 'solflare' | 'backpack'

export const WALLET_LABELS: Record<WalletProvider, string> = {
  phantom: 'Phantom',
  solflare: 'Solflare',
  backpack: 'Backpack',
}

const CONNECT_BASE: Record<WalletProvider, string> = {
  phantom: 'https://phantom.app/ul/v1/connect',
  solflare: 'https://solflare.com/ul/v1/connect',
  backpack: 'https://backpack.app/ul/v1/connect',
}

const REDIRECT = 'scematica://wallet' // deep link back into the app (see AndroidManifest)
const APP_URL = 'https://github.com/Meta-Oracle/Scematica'
const CLUSTER = 'mainnet-beta'
const STORE_KEY = 'scematica.mobilewallet'

interface Session {
  dappSecretKey: string // bs58 x25519 secret (to derive the shared secret on return)
  dappPublicKey: string // bs58 x25519 public (sent to the wallet)
  provider: WalletProvider
  walletEncryptionPublicKey?: string // bs58, from the wallet's connect response
  session?: string // opaque wallet session token (for later signing)
  address?: string // connected pubkey (base58)
}

function loadSession(): Session | null {
  if (typeof window === 'undefined') return null
  try {
    const raw = window.localStorage.getItem(STORE_KEY)
    return raw ? (JSON.parse(raw) as Session) : null
  } catch {
    return null
  }
}

function saveSession(s: Session | null) {
  if (typeof window === 'undefined') return
  try {
    if (s) window.localStorage.setItem(STORE_KEY, JSON.stringify(s))
    else window.localStorage.removeItem(STORE_KEY)
  } catch {
    /* storage disabled */
  }
}

/** The connected wallet address, if a session exists. */
export function getConnectedAddress(): string | null {
  return loadSession()?.address ?? null
}

/**
 * Begin a connect: generate an ephemeral x25519 keypair, persist it, and return the
 * wallet universal-link URL to open. The caller opens it (which launches the wallet app).
 */
export function buildConnectUrl(provider: WalletProvider): string {
  const kp = nacl.box.keyPair()
  saveSession({
    dappSecretKey: bs58.encode(kp.secretKey),
    dappPublicKey: bs58.encode(kp.publicKey),
    provider,
  })
  const params = new URLSearchParams({
    dapp_encryption_public_key: bs58.encode(kp.publicKey),
    cluster: CLUSTER,
    app_url: APP_URL,
    redirect_link: REDIRECT,
  })
  return `${CONNECT_BASE[provider]}?${params.toString()}`
}

/**
 * Handle the wallet's redirect back into the app. Decrypts the payload and returns the
 * connected address. Throws with a readable message on rejection or a bad payload.
 */
export function handleConnectRedirect(query: URLSearchParams): string {
  const sess = loadSession()
  if (!sess) throw new Error('no pending connect')

  const errCode = query.get('errorCode')
  if (errCode) throw new Error(query.get('errorMessage') || `wallet error ${errCode}`)

  // Wallets return their key as `<wallet>_encryption_public_key` (phantom/solflare/…).
  let walletPk = query.get('phantom_encryption_public_key')
  if (!walletPk) {
    const key = Array.from(query.keys()).find(k => k.endsWith('_encryption_public_key'))
    if (key) walletPk = query.get(key)
  }
  const nonce = query.get('nonce')
  const data = query.get('data')
  if (!walletPk || !nonce || !data) throw new Error('incomplete connect response')

  const shared = nacl.box.before(bs58.decode(walletPk), bs58.decode(sess.dappSecretKey))
  const decrypted = nacl.box.open.after(bs58.decode(data), bs58.decode(nonce), shared)
  if (!decrypted) throw new Error('could not decrypt wallet response')

  const payload = JSON.parse(new TextDecoder().decode(decrypted)) as {
    public_key: string
    session: string
  }
  saveSession({
    ...sess,
    walletEncryptionPublicKey: walletPk,
    session: payload.session,
    address: payload.public_key,
  })
  return payload.public_key
}

export function disconnectWallet(): void {
  saveSession(null)
}
