// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Shared setup for the issue detail/peek/widget oracle specs (NEWFRONT-121).
// Everything here is written against the driver interface only, so the same
// specs run on apps/web (oracle) and apps/web_new (parity target).
import { expect } from "../fixtures";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";
import { createIssue, deleteIssue, issueFacts, projectFacts } from "../helpers/api";

/**
 * Sign in and gate on the authenticated session. The shared scratch stack
 * throttles anonymous calls per minute, so under sibling contention the
 * password POST can be dropped while the URL wait still passes; polling the
 * driver-visible session state (with one reload-and-retry) keeps specs
 * honest instead of failing downstream on the sign-in card.
 */
export async function signIn(driver: ParityDriver, seed: ParitySeedFacts): Promise<void> {
  // The shared stack slows down under sibling contention (dev-server
  // compiles, anonymous throttle), so the whole entry sequence retries.
  let lastError: unknown = null;
  for (let attempt = 0; attempt < 3; attempt++) {
    try {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await expect.poll(() => driver.signedIn(), { timeout: 45_000 }).toBe(true);
      return;
    } catch (error) {
      lastError = error;
      await driver.page.reload().catch(() => {});
    }
  }
  throw lastError;
}

/** A seeded issue resolved live: sequences drift when siblings reset the shared stack. */
export interface SeedIssue {
  seq: string;
  id: string;
  name: string;
}

/** Resolve `seed.issueNames[index]` to its current `IDENT-seq`, id, and name. */
export async function seedIssue(seed: ParitySeedFacts, session: string, index: number): Promise<SeedIssue> {
  const { identifier } = await projectFacts(seed.workspaceSlug, seed.projectId, session);
  const name = seed.issueNames[index] as string;
  const rows = await issueFacts(seed.workspaceSlug, seed.projectId, session);
  const row = rows.find((r) => r.name === name);
  if (!row) throw new Error(`[parity] seeded issue ${JSON.stringify(name)} is missing (sibling reset?).`);
  return { seq: `${identifier}-${row.sequence_id}`, id: row.id, name };
}

/**
 * Create a scenario-owned issue and resolve its `IDENT-seq`. Owned issues
 * keep mutation scenarios isolated from sibling runs sharing the stack
 * (renames, intake moves, deletes of the seeded rows). Callers delete the
 * issue at the end of the test so sibling exact-set assertions keep passing.
 */
export async function ownIssue(
  seed: ParitySeedFacts,
  session: string,
  name: string,
  extra: Record<string, unknown> = {}
): Promise<SeedIssue> {
  const { identifier } = await projectFacts(seed.workspaceSlug, seed.projectId, session);
  const created = await createIssue(seed.workspaceSlug, seed.projectId, session, name, extra);
  return { seq: `${identifier}-${created.sequence_id}`, id: created.id, name };
}

/** Delete a scenario-owned issue (best effort: leftovers are timestamped). */
export async function dropIssue(seed: ParitySeedFacts, session: string, id: string): Promise<void> {
  await deleteIssue(seed.workspaceSlug, seed.projectId, id, session);
}
