/**
 * The Grok plugin: Scematica Omni-Agent's voice, and its ears.
 *
 * ElizaOS has no official xAI provider (`@elizaos/plugin-grok` does not exist,
 * and `@elizaos/plugin-xai@2.0.0-alpha.1` declares a `workspace:*` peer that
 * cannot resolve outside the monorepo), so this registers the model handlers
 * itself.
 *
 * It also registers something no other model plugin can: LIVE_SEARCH, backed
 * by xAI's server-side `x_search` tool. That is the difference between an
 * agent that recites training data and one that knows what was said on X ten
 * minutes ago.
 *
 * Embeddings are routed to the cortex rather than to xAI, so ElizaOS memory
 * and cortex memory occupy one vector space. See CortexClient.embed.
 */
import {
  logger,
  ModelType,
  type Action,
  type ActionResult,
  type GenerateTextParams,
  type HandlerCallback,
  type IAgentRuntime,
  type Memory,
  type ObjectGenerationParams,
  type Plugin,
  type State,
  type TextEmbeddingParams,
} from '@elizaos/core';

import { config } from '../../config.js';
import { getCortexClient } from '../cortex/client.js';
import { getGrokClient, type GrokMessage } from './client.js';

/** ElizaOS hands prompts through as one string; Grok wants a message array. */
function toMessages(params: GenerateTextParams, systemPrompt?: string): GrokMessage[] {
  const messages: GrokMessage[] = [];
  if (systemPrompt) messages.push({ role: 'system', content: systemPrompt });
  messages.push({ role: 'user', content: params.prompt });
  return messages;
}

/**
 * Pull a JSON value out of a model reply.
 *
 * Models wrap JSON in prose or fences even when told not to, and OBJECT_LARGE
 * callers in ElizaOS depend on getting a real object back. Try the strict read
 * first, then recover.
 */
export function extractJson(raw: string): unknown {
  const trimmed = raw.trim();
  try {
    return JSON.parse(trimmed);
  } catch {
    // fall through to recovery
  }

  const fenced = trimmed.match(/```(?:json)?\s*([\s\S]*?)```/);
  if (fenced?.[1]) {
    try {
      return JSON.parse(fenced[1].trim());
    } catch {
      // keep trying
    }
  }

  // Widest balanced-looking span, object or array.
  for (const [open, close] of [
    ['{', '}'],
    ['[', ']'],
  ] as const) {
    const start = trimmed.indexOf(open);
    const end = trimmed.lastIndexOf(close);
    if (start !== -1 && end > start) {
      try {
        return JSON.parse(trimmed.slice(start, end + 1));
      } catch {
        // keep trying
      }
    }
  }

  throw new Error(`Grok did not return parseable JSON. Got: ${trimmed.slice(0, 200)}`);
}

/**
 * Ask X what it is saying, right now.
 *
 * Available in conversation so the operator can say "what's the take on X?"
 * in Telegram and get a genuinely current answer, not a recollection.
 */
export const liveSearchAction: Action = {
  name: 'LIVE_SEARCH',
  similes: ['SEARCH_X', 'CHECK_TWITTER', 'WHATS_HAPPENING', 'SEARCH_TIMELINE'],
  description:
    'Search live X/Twitter discourse and the web through Grok server-side tools. Use when the ' +
    'user asks what people are saying, what is trending, or about anything recent enough that ' +
    'stored knowledge would be stale.',

  validate: async (): Promise<boolean> => getGrokClient().configured,

  handler: async (
    runtime: IAgentRuntime,
    message: Memory,
    _state?: State,
    _options?: unknown,
    callback?: HandlerCallback,
  ): Promise<ActionResult> => {
    const query = message.content?.text?.trim();
    if (!query) {
      return { success: false, text: 'Nothing to search for.', error: 'empty query' };
    }

    try {
      const result = await getGrokClient().liveSearch(
        `Search X and the web, then answer concisely and concretely.\n\nQuestion: ${query}`,
      );

      // Remember what we learned, so a later conversation on any surface can
      // build on it instead of re-searching.
      void getCortexClient().remember(result.text.slice(0, 2000), 'sense', 'observation', {
        query,
        citations: result.citations.slice(0, 10).map((citation) => citation.url),
      });

      const sourceList = result.citations
        .slice(0, 5)
        .map((citation) => `- ${citation.title ?? citation.url}`)
        .join('\n');
      const text = sourceList ? `${result.text}\n\nSources:\n${sourceList}` : result.text;

      await callback?.({ text, actions: ['LIVE_SEARCH'] });
      return {
        success: true,
        text,
        data: { citations: result.citations, query },
      };
    } catch (error) {
      const reason = (error as Error).message;
      logger.error({ error: reason }, 'LIVE_SEARCH failed');
      await callback?.({ text: `I could not reach live search just now (${reason}).` });
      return { success: false, text: 'live search failed', error: reason };
    }
  },

  examples: [
    [
      { name: '{{user}}', content: { text: 'what are people saying about the new Mojo release?' } },
      {
        name: '{{agent}}',
        content: { text: 'Checking X right now.', actions: ['LIVE_SEARCH'] },
      },
    ],
    [
      { name: '{{user}}', content: { text: 'is anything blowing up on AI twitter today?' } },
      {
        name: '{{agent}}',
        content: { text: 'Let me look at the live timeline.', actions: ['LIVE_SEARCH'] },
      },
    ],
  ],
};

export const grokPlugin: Plugin = {
  name: 'grok',
  description:
    'xAI Grok models for text and object generation, plus live X/web search via server-side tools.',

  // Ahead of other model providers if one is ever added alongside.
  priority: 100,

  config: {
    model: config.xai.model,
    baseUrl: config.xai.baseUrl,
  },

  async init(): Promise<void> {
    if (!getGrokClient().configured) {
      logger.warn(
        'grok plugin loaded without XAI_API_KEY -- text generation and live search are disabled',
      );
      return;
    }
    logger.info(`grok plugin ready (${config.xai.model})`);
  },

  actions: [liveSearchAction],

  models: {
    [ModelType.TEXT_SMALL]: async (
      runtime: IAgentRuntime,
      params: GenerateTextParams,
    ): Promise<string> =>
      getGrokClient().chat(toMessages(params, runtime.character.system), {
        model: config.xai.smallModel,
        temperature: params.temperature ?? config.xai.temperature,
        maxTokens: params.maxTokens ?? 1024,
        stopSequences: params.stopSequences ?? [],
      }),

    [ModelType.TEXT_LARGE]: async (
      runtime: IAgentRuntime,
      params: GenerateTextParams,
    ): Promise<string> =>
      getGrokClient().chat(toMessages(params, runtime.character.system), {
        model: config.xai.model,
        temperature: params.temperature ?? config.xai.temperature,
        maxTokens: params.maxTokens ?? config.xai.maxTokens,
        stopSequences: params.stopSequences ?? [],
      }),

    [ModelType.OBJECT_SMALL]: async (
      runtime: IAgentRuntime,
      params: ObjectGenerationParams,
    ): Promise<Record<string, unknown>> =>
      generateObject(runtime, params, config.xai.smallModel),

    [ModelType.OBJECT_LARGE]: async (
      runtime: IAgentRuntime,
      params: ObjectGenerationParams,
    ): Promise<Record<string, unknown>> => generateObject(runtime, params, config.xai.model),

    [ModelType.TEXT_EMBEDDING]: async (
      _runtime: IAgentRuntime,
      params: TextEmbeddingParams | string | null,
    ): Promise<number[]> => {
      const text = typeof params === 'string' ? params : (params?.text ?? '');
      const [vector] = await getCortexClient().embed([text]);
      return vector ?? [];
    },
  },
};

/**
 * Generate a structured value.
 *
 * The return type is ElizaOS's, not ours: the plugin contract declares
 * `Record<string, unknown>` for OBJECT_* handlers even though the same
 * interface accepts `output: 'array' | 'enum'`. When the caller asked for an
 * array we return the array and cast, because handing back a wrapped object
 * would satisfy the compiler and break the caller. The cast is confined to
 * this one function rather than being smeared across the plugin.
 */
async function generateObject(
  runtime: IAgentRuntime,
  params: ObjectGenerationParams,
  model: string,
): Promise<Record<string, unknown>> {
  const schemaHint = params.schema
    ? `\n\nReturn JSON matching this schema exactly:\n${JSON.stringify(params.schema)}`
    : '';
  const shapeHint =
    params.output === 'array'
      ? '\n\nReturn a JSON array.'
      : params.output === 'enum'
        ? `\n\nReturn exactly one of: ${(params.enumValues ?? []).join(', ')}`
        : '\n\nReturn a single JSON object.';

  const raw = await getGrokClient().chat(
    [
      {
        role: 'system',
        content:
          `${runtime.character.system ?? ''}\n\nYou return JSON only. No prose, no code fences.`.trim(),
      },
      { role: 'user', content: `${params.prompt}${schemaHint}${shapeHint}` },
    ],
    {
      model,
      // Structured output wants determinism far more than flair.
      temperature: params.temperature ?? 0.2,
      stopSequences: params.stopSequences ?? [],
    },
  );

  return extractJson(raw) as Record<string, unknown>;
}

export default grokPlugin;
