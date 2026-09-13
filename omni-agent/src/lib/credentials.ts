/**
 * Classifying credential failures.
 *
 * "FAIL" is not a useful diagnosis when the two likely causes need opposite
 * responses: a rejected key means find the right key, an exhausted balance
 * means the key is already correct and the problem is billing. Both xAI and
 * the X API report the second case in ways easily mistaken for the first --
 * xAI returns `permission-denied` for an out-of-credit team, and X returns
 * HTTP 402 on an otherwise perfectly valid bearer token.
 *
 * Getting this wrong costs an hour of regenerating keys that were never the
 * problem.
 */

/**
 * What a credential check concluded.
 *
 * Each arm is a different instruction to the operator, which is the whole reason this is
 * not a boolean. `rejected` sends somebody to regenerate a credential; `out-of-credit`
 * tells them not to, because the credential is fine and the bill is not; `wrong-account`
 * tells them the credential works perfectly and is signing in as somebody else — the one
 * failure where everything downstream looks healthy and the posts land where nobody is
 * watching.
 */
export type CredentialVerdict =
  | 'ok'
  | 'unset'
  | 'rejected'
  | 'out-of-credit'
  | 'wrong-account'
  | 'unknown';

export interface CredentialCheck {
  verdict: CredentialVerdict;
  /** What happened, in the provider's own words where possible. */
  detail: string;
  /** What to actually do about it. */
  remedy?: string;
}

const CREDIT_PATTERNS = [
  /credits?\s+depleted/i,
  /out of credits/i,
  /used all available credits/i,
  /monthly spending limit/i,
  /payment required/i,
  /quota.*exceed/i,
  /insufficient.*(credit|balance|funds)/i,
];

const REJECTED_PATTERNS = [
  /incorrect api key/i,
  /invalid.*(api key|token|credential)/i,
  /unauthorized/i,
  /authentication failed/i,
  /could not authenticate/i,
];

/**
 * Decide what a failure actually means.
 *
 * Credit exhaustion is checked first: xAI labels it `permission-denied`, which
 * would otherwise read as an auth rejection and send you looking for a new key
 * that works exactly as well as the one you have.
 */
export function classifyFailure(status: number | undefined, body: string): CredentialCheck {
  const text = body || '';

  if (status === 402 || CREDIT_PATTERNS.some((pattern) => pattern.test(text))) {
    return {
      verdict: 'out-of-credit',
      detail: extractMessage(text) || 'the account has no API credits available',
      remedy: 'the credential is valid -- add credits or raise the spending limit',
    };
  }

  if (status === 401 || status === 403 || REJECTED_PATTERNS.some((p) => p.test(text))) {
    return {
      verdict: 'rejected',
      detail: extractMessage(text) || `rejected with HTTP ${status ?? '?'}`,
      remedy: 'the credential itself is wrong -- regenerate it',
    };
  }

  return {
    verdict: 'unknown',
    detail: extractMessage(text) || `HTTP ${status ?? '?'}`,
  };
}

/**
 * X's numeric error codes, which name the cause far better than the HTTP status.
 *
 * The distinction that matters: code 89 blames the *access token pair*, while
 * code 32 blames the signature (so usually the consumer key/secret). A bare
 * 401 cannot tell you which of the four credentials to replace; these can.
 */
const X_ERROR_CODES: Record<number, { detail: string; remedy: string }> = {
  32: {
    detail: 'could not authenticate (signature rejected)',
    remedy:
      'the API key/secret pair is wrong, or does not match the access tokens -- check they all ' +
      'come from the same app',
  },
  64: {
    detail: 'this X account is suspended',
    remedy: 'nothing credential-side will fix this',
  },
  89: {
    detail: 'invalid or expired access token',
    remedy:
      'the API key/secret are fine; the Access Token pair is not. In the X developer portal set ' +
      'the app to "Read and write" FIRST, then regenerate Access Token and Secret -- changing ' +
      'permissions afterwards silently invalidates existing tokens',
  },
  135: {
    detail: 'timestamp out of bounds',
    remedy: 'the system clock is off by more than 5 minutes; sync it',
  },
  215: {
    detail: 'bad authentication data',
    remedy: 'one or more of the four X credentials is missing or malformed',
  },
  326: {
    detail: 'this account is temporarily locked',
    remedy: 'unlock it at x.com, then retry',
  },
};

/**
 * Classify an X failure using its numeric error code when one is present.
 *
 * Falls back to the generic classifier, so an unrecognised code still gets a
 * sensible verdict rather than nothing.
 */
export function classifyXFailure(status: number | undefined, body: string): CredentialCheck {
  let code: number | undefined;
  try {
    const parsed = JSON.parse(body) as { errors?: Array<{ code?: number }> };
    code = parsed.errors?.[0]?.code;
  } catch {
    // not JSON, or not the v1.1 error shape
  }

  if (code !== undefined && X_ERROR_CODES[code]) {
    const known = X_ERROR_CODES[code]!;
    return { verdict: 'rejected', detail: `${known.detail} (X code ${code})`, remedy: known.remedy };
  }
  return classifyFailure(status, body);
}

/** Pull the human-readable message out of a provider error body. */
export function extractMessage(body: string): string {
  if (!body.trim()) return '';
  try {
    const parsed = JSON.parse(body) as Record<string, unknown>;
    for (const key of ['error', 'detail', 'message', 'title']) {
      const value = parsed[key];
      if (typeof value === 'string' && value.trim()) return value.trim();
    }
    // X nests problems under `errors`.
    const errors = parsed.errors;
    if (Array.isArray(errors) && errors.length > 0) {
      const first = errors[0] as Record<string, unknown>;
      const message = first.message ?? first.detail ?? first.title;
      if (typeof message === 'string') return message;
    }
  } catch {
    // not JSON; fall through
  }
  return body.slice(0, 200).trim();
}
