/**
 * ScemaDEX peer-mesh relay client (TypeScript).
 *
 * A dependency-free, typed client for the `scemadex-relay` HTTP contract — the
 * same surface the Rust `RemotePeerMarket` speaks. Lets non-Rust agents and this
 * web app join the mesh: trade bonded inferences and learned experience, and
 * query the (optionally x402-gated) signal oracle.
 *
 * Wire types mirror `scemadex-sdk`'s serde encoding exactly:
 *   - `Usdc(u64)` and `Conviction(f64)` are newtypes → serialize as bare numbers.
 *   - `Address(String)` → bare string. Enums (`Venue`) → variant name string.
 *   - all amounts of USDC are micro-USDC (6 decimals).
 *
 * Works in the browser and Node 18+ (uses global `fetch`).
 *
 * @example
 *   const relay = new ScemaDexRelay("http://localhost:8080");
 *   const offer = await relay.quoteInference(intentDigest);
 *   if (offer) console.log(offer.peer_id, fromMicroUsdc(offer.price));
 */

// ── Wire types (match scemadex-sdk serde output) ────────────────────────────

export type Venue = "Raydium" | "Orca" | "Meteora" | "Jupiter" | "Custom";

/** A token amount in base units plus its mint decimals. */
export interface Amount {
  raw: number;
  decimals: number;
}

export interface RouteLeg {
  venue: Venue;
  input_mint: string;
  output_mint: string;
  /** Fraction of the parent order through this leg, in basis points (0..=10_000). */
  split_bps: number;
  expected_out: Amount;
}

export interface Route {
  legs: RouteLeg[];
  expected_out: Amount;
}

export interface Solution {
  intent_digest: string;
  route: Route;
  /** Policy self-assessed confidence, [0,1]. */
  conviction: number;
  rationale: string;
}

/** A bonded solution a peer offers for sale; `price` is micro-USDC. */
export interface InferenceOffer {
  solution: Solution;
  price: number;
  peer_id: string;
}

/** A batch of RL transitions for sale; `price` is micro-USDC, `payload` is raw bytes. */
export interface ExperienceBatch {
  peer_id: string;
  transitions: number;
  price: number;
  payload: number[];
}

export interface Reputation {
  /** 0.0 (rug-prone) … 1.0 (trusted). */
  score: number;
  samples: number;
}

export interface PoolScore {
  /** Predictive 0–100 quality score. */
  score: number;
}

export interface Advice {
  action: string;
  conviction: number;
  reason: string;
}

// ── USDC helpers (the unit of account is micro-USDC, 6 decimals) ────────────

export const toMicroUsdc = (usdc: number): number => Math.round(usdc * 1_000_000);
export const fromMicroUsdc = (micro: number): number => micro / 1_000_000;

// ── Errors ──────────────────────────────────────────────────────────────────

/** Thrown when a gated `/signal/*` endpoint returns HTTP 402 Payment Required. */
export class PaymentRequiredError extends Error {
  constructor(
    public readonly endpoint: string,
    public readonly requirements: unknown,
  ) {
    super(`x402 payment required for ${endpoint}`);
    this.name = "PaymentRequiredError";
  }
}

export class RelayError extends Error {
  constructor(
    public readonly status: number,
    public readonly endpoint: string,
    body?: string,
  ) {
    super(`relay ${endpoint} -> HTTP ${status}${body ? `: ${body}` : ""}`);
    this.name = "RelayError";
  }
}

// ── Client ────────────────────────────────────────────────────────────────

// ── x402 payment (Dexter @dexterai/x402 interop) ─────────────────────────────

/** A Solana and/or EVM signer set — structurally compatible with Dexter's `WalletSet`. */
export interface WalletSet {
  solana?: unknown;
  evm?: unknown;
}

/** Options accepted by Dexter's `payAndFetch`. */
export interface PayAndFetchOptions {
  maxAmountAtomic?: number | bigint;
  timeoutMs?: number;
  solanaRpcUrl?: string;
}

/** The discriminated result Dexter's `payAndFetch` returns. */
export interface PayResult {
  ok: boolean;
  paid?: boolean;
  response?: Response;
  amountPaid?: string | number | bigint;
  txSignature?: string;
  reason?: string;
  detail?: unknown;
}

/**
 * Pays x402-gated endpoints. Inject Dexter's `payAndFetch` here so the core
 * client stays dependency-free; [`createPaidRelay`] wires it for you.
 */
export interface X402Payer {
  wallets: WalletSet;
  options?: PayAndFetchOptions;
  payAndFetch: (
    url: string,
    init: RequestInit,
    wallets: WalletSet,
    opts: PayAndFetchOptions,
  ) => Promise<PayResult>;
}

export interface ScemaDexRelayOptions {
  /** Extra headers to send on every request (e.g. an x402 payment header). */
  headers?: Record<string, string>;
  /** Custom fetch (for Node < 18, SSR, or testing). Defaults to global fetch. */
  fetch?: typeof fetch;
  /**
   * An x402 payer for gated `/signal/*` endpoints. When set, those reads are
   * routed through `payAndFetch`, which performs the full 402 handshake (fetch →
   * sign → retry) instead of throwing [`PaymentRequiredError`].
   */
  payer?: X402Payer;
}

export class ScemaDexRelay {
  private readonly base: string;
  private readonly headers: Record<string, string>;
  private readonly doFetch: typeof fetch;
  private readonly payer?: X402Payer;

  constructor(baseUrl: string, opts: ScemaDexRelayOptions = {}) {
    this.base = baseUrl.replace(/\/+$/, "");
    this.headers = opts.headers ?? {};
    this.payer = opts.payer;
    const f = opts.fetch ?? globalThis.fetch;
    if (!f) throw new Error("No fetch available; pass opts.fetch (Node < 18).");
    this.doFetch = f.bind(globalThis);
  }

  /** Liveness check. Returns true if the relay answers "ok". */
  async health(): Promise<boolean> {
    const res = await this.doFetch(`${this.base}/health`, { headers: this.headers });
    return res.ok;
  }

  // ── Inference marketplace ──────────────────────────────────────────────

  /** Publish a bonded inference offer for sale. */
  async sellInference(offer: InferenceOffer): Promise<void> {
    await this.post("/inference/offer", offer, [204]);
  }

  /**
   * Buy the cheapest matching bonded inference for an intent digest.
   * Returns `null` if no offer exists (HTTP 404).
   */
  async quoteInference(intentDigest: string): Promise<InferenceOffer | null> {
    return this.postJson<InferenceOffer>("/inference/quote", { intent_digest: intentDigest });
  }

  // ── Experience marketplace ─────────────────────────────────────────────

  /** Publish a batch of learned experience for sale. */
  async sellExperience(batch: ExperienceBatch): Promise<void> {
    await this.post("/experience/offer", batch, [204]);
  }

  /**
   * Buy the cheapest experience batch at or under `maxPriceUsdc` (in USDC).
   * Returns `null` if none qualifies (HTTP 404).
   */
  async buyExperience(maxPriceUsdc: number): Promise<ExperienceBatch | null> {
    return this.postJson<ExperienceBatch>("/experience/buy", {
      max_price: toMicroUsdc(maxPriceUsdc),
    });
  }

  // ── Signal oracle (may be x402-gated → throws PaymentRequiredError) ─────

  reputation(mint: string): Promise<Reputation> {
    return this.getSignal<Reputation>(`/signal/reputation/${mint}`);
  }

  poolScore(mint: string): Promise<PoolScore> {
    return this.getSignal<PoolScore>(`/signal/pool_score/${mint}`);
  }

  advice(mint: string): Promise<Advice> {
    return this.getSignal<Advice>(`/signal/advice/${mint}`);
  }

  // ── internals ──────────────────────────────────────────────────────────

  private async post(path: string, body: unknown, okStatuses: number[]): Promise<Response> {
    const res = await this.doFetch(`${this.base}${path}`, {
      method: "POST",
      headers: { "content-type": "application/json", ...this.headers },
      body: JSON.stringify(body),
    });
    if (!okStatuses.includes(res.status)) {
      throw new RelayError(res.status, path, await safeText(res));
    }
    return res;
  }

  /** POST that returns a JSON body, mapping 404 → null. */
  private async postJson<T>(path: string, body: unknown): Promise<T | null> {
    const res = await this.doFetch(`${this.base}${path}`, {
      method: "POST",
      headers: { "content-type": "application/json", ...this.headers },
      body: JSON.stringify(body),
    });
    if (res.status === 404) return null;
    if (!res.ok) throw new RelayError(res.status, path, await safeText(res));
    return (await res.json()) as T;
  }

  private async getSignal<T>(path: string): Promise<T> {
    const url = `${this.base}${path}`;

    // With an x402 payer, route the gated read through payAndFetch — it does the
    // full 402 handshake (fetch → sign payment → retry) and returns the merchant
    // response, so the caller never sees a PaymentRequiredError.
    if (this.payer) {
      const result = await this.payer.payAndFetch(
        url,
        { method: "GET", headers: this.headers },
        this.payer.wallets,
        this.payer.options ?? {},
      );
      if (!result.ok || !result.response) {
        throw new RelayError(402, path, result.reason ?? "x402 payment failed");
      }
      return (await result.response.json()) as T;
    }

    const res = await this.doFetch(url, { headers: this.headers });
    if (res.status === 402) {
      throw new PaymentRequiredError(path, await safeJson(res));
    }
    if (!res.ok) throw new RelayError(res.status, path, await safeText(res));
    return (await res.json()) as T;
  }
}

// ── Dexter x402 wiring ───────────────────────────────────────────────────────

/** The subset of Dexter's `@dexterai/x402/client` module that {@link createPaidRelay} uses. */
export interface X402ClientModule {
  payAndFetch: (url: string, init: RequestInit, wallets: any, opts: any) => Promise<PayResult>;
  createKeypairWallet: (secretKey: string) => Promise<unknown>;
  createEvmKeypairWallet?: (secretKey: string) => Promise<unknown>;
}

/**
 * Build a relay client that automatically pays x402-gated signal endpoints with
 * the [Dexter x402 SDK](https://github.com/Dexter-DAO/dexter-x402-sdk). Keeps the
 * core client dependency-free by taking the Dexter client module by injection:
 *
 * ```ts
 * import * as x402 from "@dexterai/x402/client";
 * import { createPaidRelay } from "./lib/scemadex";
 *
 * const relay = await createPaidRelay("http://localhost:8080", x402, {
 *   solanaPrivateKey: process.env.SOLANA_PRIVATE_KEY!,
 * });
 * const rep = await relay.reputation(mint); // the 402 is paid transparently
 * ```
 */
export async function createPaidRelay(
  baseUrl: string,
  x402: X402ClientModule,
  secrets: { solanaPrivateKey?: string; evmPrivateKey?: string },
  opts: {
    payOptions?: PayAndFetchOptions;
    headers?: Record<string, string>;
    fetch?: typeof fetch;
  } = {},
): Promise<ScemaDexRelay> {
  const wallets: WalletSet = {};
  if (secrets.solanaPrivateKey) {
    wallets.solana = await x402.createKeypairWallet(secrets.solanaPrivateKey);
  }
  if (secrets.evmPrivateKey && x402.createEvmKeypairWallet) {
    wallets.evm = await x402.createEvmKeypairWallet(secrets.evmPrivateKey);
  }
  return new ScemaDexRelay(baseUrl, {
    headers: opts.headers,
    fetch: opts.fetch,
    payer: {
      wallets,
      options: opts.payOptions,
      payAndFetch: x402.payAndFetch as X402Payer["payAndFetch"],
    },
  });
}

async function safeText(res: Response): Promise<string | undefined> {
  try {
    return await res.text();
  } catch {
    return undefined;
  }
}

async function safeJson(res: Response): Promise<unknown> {
  try {
    return await res.json();
  } catch {
    return undefined;
  }
}
