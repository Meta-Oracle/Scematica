/**
 * GrokClient behaviour, driven entirely through an injected fetch.
 *
 * These run without an API key and without network access, which matters:
 * the retry and parsing logic is exactly the code that is hardest to exercise
 * against the live API and most likely to misbehave when it finally matters.
 */
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import { GrokClient, GrokError, parseResponsesOutput } from './client.js';

type FetchArgs = { url: string; body: unknown };

function mockFetch(
  responder: (call: number, args: FetchArgs) => { status?: number; json?: unknown; body?: string },
): { fetch: typeof fetch; calls: FetchArgs[] } {
  const calls: FetchArgs[] = [];
  const impl = (async (url: string | URL | Request, init?: RequestInit) => {
    const args: FetchArgs = {
      url: String(url),
      body: init?.body ? JSON.parse(String(init.body)) : undefined,
    };
    calls.push(args);
    const result = responder(calls.length, args);
    const status = result.status ?? 200;
    return {
      ok: status >= 200 && status < 300,
      status,
      statusText: status === 200 ? 'OK' : 'Error',
      json: async () => result.json ?? {},
      text: async () => result.body ?? '',
    } as Response;
  }) as unknown as typeof fetch;
  return { fetch: impl, calls };
}

describe('GrokClient.chat', () => {
  it('sends an OpenAI-shaped body and returns the content', async () => {
    const { fetch, calls } = mockFetch(() => ({
      json: { choices: [{ message: { content: 'hello from grok' } }] },
    }));
    const client = new GrokClient('xai-test', 'https://api.x.ai/v1', fetch);

    const reply = await client.chat([{ role: 'user', content: 'hi' }], { temperature: 0.4 });

    assert.equal(reply, 'hello from grok');
    assert.equal(calls[0]!.url, 'https://api.x.ai/v1/chat/completions');
    const body = calls[0]!.body as Record<string, unknown>;
    assert.equal(body.temperature, 0.4);
    assert.deepEqual(body.messages, [{ role: 'user', content: 'hi' }]);
  });

  it('throws a clear error when the response carries no content', async () => {
    const { fetch } = mockFetch(() => ({ json: { choices: [] } }));
    const client = new GrokClient('xai-test', 'https://api.x.ai/v1', fetch);
    await assert.rejects(() => client.chat([{ role: 'user', content: 'hi' }]), /no message content/);
  });

  it('refuses to call the API at all without a key', async () => {
    const { fetch, calls } = mockFetch(() => ({ json: {} }));
    const client = new GrokClient('', 'https://api.x.ai/v1', fetch);
    await assert.rejects(() => client.chat([{ role: 'user', content: 'hi' }]), /not configured/);
    assert.equal(calls.length, 0, 'must not issue a request it knows will fail');
  });
});

describe('GrokClient retry policy', () => {
  it('retries a 429 and succeeds', async () => {
    const { fetch, calls } = mockFetch((call) =>
      call < 3
        ? { status: 429, body: 'rate limited' }
        : { json: { choices: [{ message: { content: 'eventually' } }] } },
    );
    const client = new GrokClient('xai-test', 'https://api.x.ai/v1', fetch);

    const reply = await client.chat([{ role: 'user', content: 'hi' }]);
    assert.equal(reply, 'eventually');
    assert.equal(calls.length, 3);
  });

  it('does not retry a 401, because the key will not fix itself', async () => {
    const { fetch, calls } = mockFetch(() => ({ status: 401, body: 'Incorrect API key provided' }));
    const client = new GrokClient('xai-bad', 'https://api.x.ai/v1', fetch);

    await assert.rejects(
      () => client.chat([{ role: 'user', content: 'hi' }]),
      (error: GrokError) => {
        assert.equal(error.status, 401);
        assert.equal(error.retryable, false);
        assert.match(error.body ?? '', /Incorrect API key/);
        return true;
      },
    );
    assert.equal(calls.length, 1, 'a bad key must not be retried');
  });

  it('gives up after the attempt budget on persistent 5xx', async () => {
    const { fetch, calls } = mockFetch(() => ({ status: 503, body: 'upstream down' }));
    const client = new GrokClient('xai-test', 'https://api.x.ai/v1', fetch);
    await assert.rejects(() => client.chat([{ role: 'user', content: 'hi' }]), /503/);
    assert.equal(calls.length, 3);
  });
});

describe('GrokClient.liveSearch', () => {
  it('targets the responses endpoint and enables x_search by default', async () => {
    const { fetch, calls } = mockFetch(() => ({
      json: { output_text: 'people are discussing kernels', output: [] },
    }));
    const client = new GrokClient('xai-test', 'https://api.x.ai/v1', fetch);

    const result = await client.liveSearch('what is being said about mojo kernels?');

    assert.equal(calls[0]!.url, 'https://api.x.ai/v1/responses');
    const body = calls[0]!.body as Record<string, unknown>;
    assert.deepEqual(body.tools, [{ type: 'x_search' }]);
    assert.equal(body.stream, false);
    assert.deepEqual(body.input, [
      { role: 'user', content: 'what is being said about mojo kernels?' },
    ]);
    assert.equal(result.text, 'people are discussing kernels');
  });

  it('passes through additional server-side tools when asked', async () => {
    const { fetch, calls } = mockFetch(() => ({ json: { output_text: 'ok' } }));
    const client = new GrokClient('xai-test', 'https://api.x.ai/v1', fetch);
    await client.liveSearch('q', { tools: ['x_search', 'web_search'] });
    const body = calls[0]!.body as Record<string, unknown>;
    assert.deepEqual(body.tools, [{ type: 'x_search' }, { type: 'web_search' }]);
  });
});

describe('parseResponsesOutput', () => {
  it('reads the documented output_text plus nested citations', () => {
    const { text, citations } = parseResponsesOutput({
      output_text: 'The consensus is that it is slower than numpy.',
      output: [
        {
          type: 'message',
          content: [{ type: 'output_text', text: 'The consensus is that it is slower than numpy.' }],
        },
        {
          type: 'x_search_call',
          citations: [
            { url: 'https://x.com/a/status/1', title: 'a post', snippet: 'numpy wins here' },
            { url: 'https://x.com/b/status/2' },
          ],
        },
      ],
    });

    assert.equal(text, 'The consensus is that it is slower than numpy.');
    assert.equal(citations.length, 2);
    assert.equal(citations[0]!.title, 'a post');
    assert.equal(citations[0]!.snippet, 'numpy wins here');
  });

  it('handles bare string citations', () => {
    const { citations } = parseResponsesOutput({
      output_text: 'hi',
      citations: ['https://x.com/one', 'https://x.com/two', 'not-a-url'],
    });
    assert.deepEqual(
      citations.map((c) => c.url),
      ['https://x.com/one', 'https://x.com/two'],
    );
  });

  it('de-duplicates citations reached by more than one path', () => {
    const { citations } = parseResponsesOutput({
      citations: [{ url: 'https://x.com/dup' }],
      output: [{ sources: [{ url: 'https://x.com/dup' }, { url: 'https://x.com/other' }] }],
    });
    assert.equal(citations.length, 2);
  });

  it('falls back to a chat-completions shape', () => {
    const { text } = parseResponsesOutput({
      choices: [{ message: { content: 'chat shaped reply' } }],
    });
    assert.equal(text, 'chat shaped reply');
  });

  it('returns empty rather than throwing on an unfamiliar payload', () => {
    for (const payload of [null, undefined, 42, 'a string', {}, { output: [{ weird: true }] }]) {
      const result = parseResponsesOutput(payload);
      assert.equal(typeof result.text, 'string');
      assert.ok(Array.isArray(result.citations));
    }
  });

  it('does not recurse forever on a self-referential payload', () => {
    const cyclic: Record<string, unknown> = { output_text: 'fine' };
    cyclic.self = cyclic;
    const { text } = parseResponsesOutput(cyclic);
    assert.equal(text, 'fine');
  });
});
