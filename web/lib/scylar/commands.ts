// Slash commands.
//
// Two jobs. The obvious one is local actions — clearing the transcript, toggling
// context — that should never cost an LLM call. The less obvious one is that `/status`
// and its siblings *force* the state block on and ask a question phrased to use it. The
// difference between "how's the bot doing" with context off and the same question with
// it on is the difference between a plausible paragraph and a true one, and nobody
// should have to remember to flip a toggle first.
//
// Parsing is deliberately conservative: a message is a command only when its first word
// matches a known name exactly. "/tmp is fine on Linux" is a sentence, and treating it
// as a failed command would be worse than sending it.

export interface CommandSpec {
  name: string
  args?: string
  help: string
}

export const COMMANDS: CommandSpec[] = [
  { name: '/help', help: 'List these commands.' },
  { name: '/status', help: 'Read the live bot state and summarise it.' },
  { name: '/positions', help: 'Walk through every open position.' },
  { name: '/filters', help: 'Explain what the filter pipeline is rejecting and why.' },
  { name: '/nn', help: 'Report on the Deep Q* agent — training, ε, whether it is gating.' },
  { name: '/context', args: 'on|off', help: 'Attach live bot state to each message.' },
  { name: '/retry', help: 'Send the last message again.' },
  { name: '/clear', help: 'Wipe the transcript and start a new session.' },
]

export type Command =
  /** Not a command. Send the text as written. */
  | { kind: 'none' }
  | { kind: 'help' }
  | { kind: 'clear' }
  | { kind: 'retry' }
  | { kind: 'context'; enabled: boolean | 'toggle' }
  /** A question that only makes sense against live state, so it forces it on. */
  | { kind: 'ask'; prompt: string }

/** Prompts behind the state-backed commands. Phrased to lean on the block, not the model. */
const ASKS: Record<string, string> = {
  '/status':
    'Summarise the current state of the bot from the state block: is it running, ' +
    'session PnL, how many trades, anything that looks wrong. Be brief.',
  '/positions':
    'Go through each open position in the state block. For each one give the move, how ' +
    'long it has been held, and where its TP and SL currently sit. If there are none, ' +
    'say so in one line.',
  '/filters':
    'From the state block, what is the filter pipeline rejecting most, and what does ' +
    'each of those rejection reasons actually mean? Say whether the pass rate looks ' +
    'healthy for a new-pool sniper.',
  '/nn':
    'Report on the Deep Q* agent from the state block: training steps, epsilon, replay ' +
    'size, average loss, and whether it is gating buys yet. Say what stage of training ' +
    'that puts it at.',
}

export function parseCommand(input: string): Command {
  const text = input.trim()
  if (!text.startsWith('/')) return { kind: 'none' }

  const [head, ...rest] = text.split(/\s+/)
  const name = head.toLowerCase()

  switch (name) {
    case '/help':
      return { kind: 'help' }
    case '/clear':
      return { kind: 'clear' }
    case '/retry':
      return { kind: 'retry' }
    case '/context': {
      const arg = (rest[0] || '').toLowerCase()
      if (arg === 'on') return { kind: 'context', enabled: true }
      if (arg === 'off') return { kind: 'context', enabled: false }
      return { kind: 'context', enabled: 'toggle' }
    }
    default:
      return name in ASKS ? { kind: 'ask', prompt: ASKS[name] } : { kind: 'none' }
  }
}

/** The `/help` text, rendered through the same markdown path as a model reply. */
export function helpText(): string {
  const rows = COMMANDS.map(
    (c) => `- \`${c.name}${c.args ? ' ' + c.args : ''}\` — ${c.help}`,
  ).join('\n')

  return [
    '### Commands',
    rows,
    '',
    'Anything else is sent to the model. With context on, each message carries a live ' +
      'read of the bot; with it off, I have no idea what your sniper is doing.',
  ].join('\n')
}
