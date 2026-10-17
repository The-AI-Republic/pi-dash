// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: code-review links list, attach and detach against live
// Django. The provider snapshot is best-effort server-side, so the
// suite runs with no Git provider reachable.
import { describe, expect, it } from "vitest";
import { createIssue, deleteIssue } from "../contracts/issue-actions.js";
import { attachCodeReview, detachCodeReview, listCodeReviews } from "../contracts/issue-code-reviews.js";
import { listIssues } from "../contracts/issues.js";
import { listProjects } from "../contracts/projects.js";
import { CONTRACT_PROJECT_IDENTIFIER, CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";
import type { ApiClient } from "../client.js";

const PROBE_NAME = "Contract review probe";
const REVIEW_URL = "https://github.com/contract-org/contract-repo/pull/42";

async function seededProjectId(client: ApiClient): Promise<string> {
  const projects = await listProjects(client, CONTRACT_WORKSPACE);
  const seeded = projects.find((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER);
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

async function freshProbe(client: ApiClient, projectId: string): Promise<string> {
  const page = await listIssues(client, CONTRACT_WORKSPACE, projectId);
  for (const row of page.results.filter((entry) => entry.name === PROBE_NAME)) {
    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, row.id);
  }
  const probe = await createIssue(client, CONTRACT_WORKSPACE, projectId, { name: PROBE_NAME });
  return probe.id;
}

describe.skipIf(!contractEnabled)("code reviews contract", () => {
  it("attaches, lists, re-attaches idempotently and detaches a review", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const probeId = await freshProbe(client, projectId);

    const attached = await attachCodeReview(client, CONTRACT_WORKSPACE, projectId, probeId, REVIEW_URL);
    expect(attached.url).toBe(REVIEW_URL);
    expect(attached.external_iid).toBe("42");
    expect(attached.issue).toBe(probeId);

    const listed = await listCodeReviews(client, CONTRACT_WORKSPACE, projectId, probeId);
    expect(listed.map((row) => row.id)).toContain(attached.id);

    const reattached = await attachCodeReview(client, CONTRACT_WORKSPACE, projectId, probeId, REVIEW_URL);
    expect(reattached.id).toBe(attached.id);

    await detachCodeReview(client, CONTRACT_WORKSPACE, projectId, probeId, attached.id);
    const cleared = await listCodeReviews(client, CONTRACT_WORKSPACE, projectId, probeId);
    expect(cleared.map((row) => row.id)).not.toContain(attached.id);

    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, probeId);
  });

  it("rejects an unsupported review URL with 400", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const probeId = await freshProbe(client, projectId);

    await expect(
      attachCodeReview(client, CONTRACT_WORKSPACE, projectId, probeId, "not-a-review-url")
    ).rejects.toMatchObject({ status: 400 });

    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, probeId);
  });
});
