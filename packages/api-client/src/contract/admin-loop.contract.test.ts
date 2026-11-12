// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: instance-admin loop jobs CRUD plus the targets read against
// live Django. Jobs are throwaway per run (unique slugs); targets have no
// write API, so the suite proves the read and its skip-reason filter on
// the fresh job's empty edge set. Signs in through the native admin form
// (see setup sharedAdminSignedIn) for the admin-session-id cookie.
import { describe, expect, it } from "vitest";
import { ApiError } from "../errors.js";
import {
  LOOP_SKIP_REASONS,
  createLoopJob,
  deleteLoopJob,
  getLoopJob,
  listLoopJobs,
  listLoopTargets,
  updateLoopJob,
} from "../contracts/admin-loop.js";
import { contractEnabled, sharedAdminSignedIn } from "./setup.js";
import type { ApiClient } from "../client.js";

async function signedIn(): Promise<ApiClient> {
  return sharedAdminSignedIn();
}

/** Fresh slug per run: lowercase hyphens only (backend slug rule). */
function lifecycleSlug(): string {
  const suffix = `${Date.now().toString(36)}${Math.floor(Math.random() * 1296).toString(36)}`;
  return `ct-loop-${suffix}`.toLowerCase().replace(/[^a-z0-9-]/g, "-");
}

describe.skipIf(!contractEnabled)("loop job lifecycle contract", () => {
  it("creates, reads, updates and deletes a job", async () => {
    const client = await signedIn();
    const slug = lifecycleSlug();
    const created = await createLoopJob(client, {
      slug,
      name: `Contract ${slug}`,
      public_name: `Contract ${slug} public`,
      public_description: "Seeded by the contract suite.",
      prompt: "Summarize the workspace.",
      min_role: 15,
      rrule: "FREQ=DAILY;BYHOUR=2",
      tzid: "UTC",
    });
    try {
      expect(created.slug).toBe(slug);
      expect(created.is_builtin).toBe(false);
      expect(created.enabled).toBe(true);

      const jobs = await listLoopJobs(client);
      expect(jobs.some((job) => job.slug === slug)).toBe(true);

      const detail = await getLoopJob(client, created.id);
      expect(detail.slug).toBe(slug);
      expect(detail.prompt.length).toBeGreaterThan(0);
      expect(detail.stats.target_count).toBe(0);
      expect(detail.stats.completed).toBe(0);

      const disabled = await updateLoopJob(client, created.id, { enabled: false });
      expect(disabled.enabled).toBe(false);
      const reenabled = await updateLoopJob(client, created.id, { enabled: true });
      expect(reenabled.enabled).toBe(true);

      const renamed = await updateLoopJob(client, created.id, { name: `Renamed ${slug}` });
      expect(renamed.name).toBe(`Renamed ${slug}`);
    } finally {
      await deleteLoopJob(client, created.id).catch(() => null);
    }
    const gone = await getLoopJob(client, created.id).then(
      () => null,
      (caught: unknown) => caught
    );
    expect(gone).toBeInstanceOf(ApiError);
    expect((gone as ApiError).status).toBe(404);
  });

  it("rejects a duplicate slug with 409", async () => {
    const client = await signedIn();
    const slug = lifecycleSlug();
    const created = await createLoopJob(client, {
      slug,
      name: `Contract ${slug}`,
      public_name: `Contract ${slug} public`,
      prompt: "Do it.",
      rrule: "FREQ=DAILY;BYHOUR=2",
    });
    try {
      const error = await createLoopJob(client, {
        slug,
        name: "Dupe",
        public_name: "Dupe",
        prompt: "Do it.",
        rrule: "FREQ=DAILY;BYHOUR=2",
      }).then(
        () => null,
        (caught: unknown) => caught
      );
      expect(error).toBeInstanceOf(ApiError);
      expect((error as ApiError).status).toBe(409);
    } finally {
      await deleteLoopJob(client, created.id).catch(() => null);
    }
  });

  it("rejects a malformed slug and a sub-hourly rule with 400", async () => {
    const client = await signedIn();
    const badSlug = await createLoopJob(client, {
      slug: "Bad Slug",
      name: "Bad",
      public_name: "Bad",
      prompt: "p",
      rrule: "FREQ=DAILY",
    }).then(
      () => null,
      (caught: unknown) => caught
    );
    expect(badSlug).toBeInstanceOf(ApiError);
    expect((badSlug as ApiError).status).toBe(400);

    const fast = await createLoopJob(client, {
      slug: lifecycleSlug(),
      name: "Fast",
      public_name: "Fast",
      prompt: "p",
      rrule: "FREQ=MINUTELY",
    }).then(
      () => null,
      (caught: unknown) => caught
    );
    expect(fast).toBeInstanceOf(ApiError);
    expect((fast as ApiError).status).toBe(400);
  });
});

describe.skipIf(!contractEnabled)("loop targets contract", () => {
  it("reads an empty targets page and accepts the skip-reason filter", async () => {
    const client = await signedIn();
    const slug = lifecycleSlug();
    const created = await createLoopJob(client, {
      slug,
      name: `Contract ${slug}`,
      public_name: `Contract ${slug} public`,
      prompt: "Summarize.",
      rrule: "FREQ=DAILY;BYHOUR=2",
    });
    try {
      const page = await listLoopTargets(client, created.id);
      expect(page.page).toBe(1);
      expect(page.results).toEqual([]);

      // Every documented reason is accepted (empty set, same shape).
      for (const reason of LOOP_SKIP_REASONS) {
        const filtered = await listLoopTargets(client, created.id, { skipReason: reason });
        expect(filtered.page).toBe(1);
        expect(filtered.results).toEqual([]);
      }
      // Blank resets to all (no skip_reason param sent).
      const reset = await listLoopTargets(client, created.id, { skipReason: "" });
      expect(reset.results).toEqual([]);
    } finally {
      await deleteLoopJob(client, created.id).catch(() => null);
    }
  });

  it("answers 404 for an unknown job's targets", async () => {
    const client = await signedIn();
    const error = await listLoopTargets(client, "00000000-0000-0000-0000-000000000000").then(
      () => null,
      (caught: unknown) => caught
    );
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).status).toBe(404);
  });
});
