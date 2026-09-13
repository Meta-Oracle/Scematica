/**
 * Create the database schema before the runtime asks for a row in it.
 *
 * ## The upstream bug this works around
 *
 * `@elizaos/core` 1.7.2's `AgentRuntime.initialize` does this, in this order:
 *
 * ```js
 * if (!await this.adapter.isReady()) await this.adapter.init();
 * const existingAgent = await this.ensureAgentExists({ ...character });   // SELECT FROM agents
 * await Promise.all([ ..., this.runPluginMigrations() ]);                  // CREATE TABLE agents
 * ```
 *
 * The tables are created *after* the first query against them. On any database that
 * already has the schema, the ordering is invisible. On a fresh one it cannot work, and
 * the failure is `relation "agents" does not exist` from a query whose SQL looks perfectly
 * reasonable — so it reads as a driver or connection problem rather than as an empty
 * database.
 *
 * Two things would normally save it and neither does:
 *
 * - `adapter.init()` would create the schema, except that `PGliteDatabaseAdapter.init()`
 *   is a **no-op that only logs**. The schema lives in `plugin-sql`'s `schema` export and
 *   reaches the database exclusively through `runPluginMigrations`.
 * - `isReady()` would at least force `init()` to run, except that for PGlite it is
 *   `!manager.isShuttingDown()` — true the instant the manager is constructed, whether or
 *   not a single table exists. So `init()` is never called on this path anyway.
 *
 * Deleting the data directory does not help: a fresh boot fails identically. Verified
 * against 1.7.2 on 2026-09-13.
 *
 * ## Why this is safe to do ourselves
 *
 * `createDatabaseAdapter` keeps its PGlite client manager in a module-level singleton, so
 * an adapter made here and the one the runtime makes a moment later are the **same
 * connection to the same database**. Migrating through this one lands the schema exactly
 * where the runtime will look for it.
 *
 * Only `plugin-sql`'s own schema is applied. That is deliberate and minimal: it is the one
 * carrying `agents`, which is all `ensureAgentExists` needs, and core's own
 * `runPluginMigrations` still runs afterwards to pick up every other plugin's schema. The
 * migrator is idempotent, so this costs one no-op pass on every subsequent boot.
 *
 * Do NOT close the adapter here. Closing marks the shared singleton as shutting down, and
 * the runtime then inherits a manager that refuses work.
 */
import { logger } from '@elizaos/core';

/**
 * Imported dynamically and typed by hand.
 *
 * `@elizaos/plugin-sql`'s `package.json` points its `exports` at the built JS without a
 * matching `types` condition, so a static import resolves to an implicit `any` and fails
 * the typecheck. Declaring the two members actually used is narrower than widening the
 * whole module, and it documents exactly what this workaround depends on — which is the
 * part that will break when the upstream ordering bug is fixed.
 */
interface SqlModule {
  createDatabaseAdapter: (
    config: { dataDir?: string; postgresUrl?: string },
    agentId: string,
  ) => { runPluginMigrations?: (plugins: unknown[], options?: unknown) => Promise<void> };
  plugin: unknown;
}

/**
 * Ensure the core tables exist.
 *
 * Never throws: if this cannot run, the runtime's own attempt is still ahead and its error
 * is the more informative one. Swallowing the failure here and letting the real boot
 * report it beats replacing a precise message with ours.
 */
export async function ensureSchema(agentId: string): Promise<void> {
  try {
    // The specifier is a variable so TypeScript does not try to resolve the module's
    // types: `plugin-sql`'s `exports` map has no `types` condition, so a literal import —
    // static or dynamic — fails with TS7016 even though the declarations exist on disk.
    // `SqlModule` above is the contract this file actually relies on.
    const specifier = '@elizaos/plugin-sql';
    const sql = (await import(specifier)) as SqlModule;
    const adapter = sql.createDatabaseAdapter({}, agentId);
    if (typeof adapter.runPluginMigrations !== 'function') {
      // A future version that fixed the ordering, or a different adapter shape. Leave it
      // alone rather than guessing at a new API.
      return;
    }
    await adapter.runPluginMigrations([sql.plugin], { verbose: false });
  } catch (error) {
    logger.debug(
      { error: (error as Error).message },
      'pre-boot schema check did not complete; the runtime will try on its own',
    );
  }
}
