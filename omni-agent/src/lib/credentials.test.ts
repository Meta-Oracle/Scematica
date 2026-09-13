/**
 * Credential failure classification.
 *
 * Every payload here is a real response captured from the live APIs during
 * setup, not an invented one. Both providers report an exhausted balance in
 * ways that read as auth failures, which is precisely the confusion this
 * module exists to prevent.
 */
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import { classifyFailure, classifyXFailure, extractMessage } from './credentials.js';

describe('classifyFailure', () => {
  it('reads xAI credit exhaustion as billing, not a bad key', () => {
    // Real xAI response. Note the code says "permission-denied", which is
    // exactly why a naive reading sends you to regenerate a working key.
    const body = JSON.stringify({
      code: 'permission-denied',
      error:
        'Your team a1175d96-0667-4554-8c8c-82cb819c995e has either used all available credits ' +
        'or reached its monthly spending limit. To continue making API requests, please ' +
        'purchase more credits or raise your spending limit.',
    });

    const result = classifyFailure(403, body);
    assert.equal(result.verdict, 'out-of-credit');
    assert.match(result.remedy ?? '', /valid/);
    assert.match(result.detail, /credits/);
  });

  it('reads an X 402 as billing', () => {
    // Real X response against a valid bearer token.
    const body = JSON.stringify({
      detail: 'credits depleted',
      status: 402,
      title: 'Payment Required',
      type: 'https://api.x.com/2/problems/credits-depleted',
    });

    const result = classifyFailure(402, body);
    assert.equal(result.verdict, 'out-of-credit');
    assert.equal(result.detail, 'credits depleted');
  });

  it('reads a genuinely bad xAI key as rejected', () => {
    // Real xAI response to the previous, invalid key.
    const body = JSON.stringify({
      code: 'invalid-argument',
      error: 'Incorrect API key provided. You can obtain an API key from https://console.x.ai.',
    });

    const result = classifyFailure(400, body);
    assert.equal(result.verdict, 'rejected');
    assert.match(result.remedy ?? '', /regenerate/);
  });

  it('treats a bare 401 as rejected even with an empty body', () => {
    assert.equal(classifyFailure(401, '').verdict, 'rejected');
  });

  it('does not guess when the failure is unrecognisable', () => {
    const result = classifyFailure(500, 'upstream exploded');
    assert.equal(result.verdict, 'unknown');
    assert.equal(result.remedy, undefined, 'an unknown cause must not suggest a remedy');
  });

  it('prioritises credit signals over auth signals when both appear', () => {
    // A 403 whose body is about money is about money.
    const result = classifyFailure(403, JSON.stringify({ error: 'out of credits, unauthorized' }));
    assert.equal(result.verdict, 'out-of-credit');
  });
});

describe('extractMessage', () => {
  it('finds the message under any of the common keys', () => {
    assert.equal(extractMessage(JSON.stringify({ error: 'a' })), 'a');
    assert.equal(extractMessage(JSON.stringify({ detail: 'b' })), 'b');
    assert.equal(extractMessage(JSON.stringify({ message: 'c' })), 'c');
    assert.equal(extractMessage(JSON.stringify({ title: 'd' })), 'd');
  });

  it('unwraps the X errors array', () => {
    const body = JSON.stringify({ errors: [{ message: 'Could not authenticate you' }] });
    assert.equal(extractMessage(body), 'Could not authenticate you');
  });

  it('falls back to raw text for non-JSON bodies', () => {
    assert.equal(extractMessage('plain text failure'), 'plain text failure');
    assert.equal(extractMessage(''), '');
  });
});

describe('classifyXFailure', () => {
  it('blames the access token pair on code 89, not the API key', () => {
    // Real response from the supplied access token pair.
    const body = JSON.stringify({ errors: [{ code: 89, message: 'Invalid or expired token.' }] });
    const result = classifyXFailure(401, body);

    assert.equal(result.verdict, 'rejected');
    assert.match(result.detail, /invalid or expired access token/i);
    assert.match(result.remedy ?? '', /API key\/secret are fine/);
    assert.match(result.remedy ?? '', /Read and write.*FIRST/s);
  });

  it('blames the signature on code 32', () => {
    const body = JSON.stringify({ errors: [{ code: 32, message: 'Could not authenticate you.' }] });
    const result = classifyXFailure(401, body);
    assert.match(result.detail, /signature rejected/);
    assert.match(result.remedy ?? '', /same app/);
  });

  it('names a clock problem on code 135 rather than blaming credentials', () => {
    const body = JSON.stringify({ errors: [{ code: 135, message: 'Timestamp out of bounds.' }] });
    const result = classifyXFailure(401, body);
    assert.match(result.remedy ?? '', /clock/);
  });

  it('falls back to the generic classifier for unknown codes', () => {
    const body = JSON.stringify({ errors: [{ code: 99999, message: 'something new' }] });
    assert.equal(classifyXFailure(401, body).verdict, 'rejected');
    // Billing still wins even when wrapped in the v1.1 error shape.
    assert.equal(classifyXFailure(402, '{"detail":"credits depleted"}').verdict, 'out-of-credit');
  });
});
