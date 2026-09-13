/**
 * Test preload: redirect all agent state to a throwaway directory.
 *
 * Loaded via `--import` so it runs before any test file imports `config.ts`,
 * which reads these variables once at module load.
 *
 * This exists because the suite was writing into the real queue: a `doctor`
 * run after `npm test` reported "pending 1, posted 3" for drafts that were
 * test fixtures. Operator state is the one thing in this system that cannot
 * be regenerated -- the proposal log is the cortex's replayable training set
 * -- so tests get their own sandbox rather than sharing it.
 */
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const sandbox = mkdtempSync(join(tmpdir(), 'scema-test-'));

process.env.SCEMA_DATA_DIR = sandbox;
process.env.SCEMA_QUEUE_PATH = join(sandbox, 'queue', 'proposals.jsonl');

// Keep tests off the network and away from any real account, whatever the
// developer happens to have in their .env.
process.env.SCEMA_DRY_RUN = 'true';
process.env.TWITTER_ACCESS_TOKEN = '';
process.env.TWITTER_ACCESS_TOKEN_SECRET = '';
