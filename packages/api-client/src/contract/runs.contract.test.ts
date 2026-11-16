// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: run list/get/create/cancel, issue dispatch, and re-tick
// against live Django. The cancel test consumes a seed probe; re-run
// contract/seed-contracts.py before re-running this suite.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import { ApiError } from "../errors.js";
import { listIssues } from "../contracts/issues.js";
import { listProjects } from "../contracts/projects.js";
import { cancelRun, commentAndRun, createRun, getRun, listRuns, reTickIssue, runAi } from "../contracts/runs.js";
import { listWorkspaces } from "../contracts/workspaces.js";
import { CONTRACT_PROJECT_IDENTIFIER, CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";

async function seededWorkspaceId(client: ApiClient): Promise<string> {
  const workspaces = await listWorkspaces(client);
  const seeded = workspaces.find((workspace) => workspace.slug === CONTRACT_WORKSPACE);
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

async function seededProjectId(client: ApiClient): Promise<string> {
  const projects = await listProjects(client, CONTRACT_WORKSPACE);
  const seeded = projects.find((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER);
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

async function seededIssueId(client: ApiClient): Promise<string> {
  const page = await listIssues(client, CONTRACT_WORKSPACE, await seededProjectId(client));
  const seeded = page.results.find((issue) => issue.name === "Contract issue one");
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

describe.skipIf(!contractEnabled)("runs contract", () => {
  it("lists workspace runs newest-first in a page envelope", async () => {
    const client = await sharedSignedIn();
    const page = await listRuns(client, { workspaceId: await seededWorkspaceId(client) });
    expect(page.page).toBe(1);
    expect(page.per_page).toBe(30);
    expect(page.count).toBe(page.results.length);
    expect(page.results.some((run) => run.prompt === "contract-seed-run")).toBe(true);
  });

  it("scopes the list to one project", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const page = await listRuns(client, { projectId });
    expect(page.results.length).toBeGreaterThan(0);
    expect(page.results.every((run) => run.pod_detail.project === projectId)).toBe(true);
  });

  it("reads one run without events unless asked", async () => {
    const client = await sharedSignedIn();
    const page = await listRuns(client, { workspaceId: await seededWorkspaceId(client) });
    const seeded = page.results.find((run) => run.prompt === "contract-seed-run");
    expect(seeded).toBeDefined();
    const detail = await getRun(client, seeded?.id ?? "");
    expect(detail.prompt).toBe("contract-seed-run");
    expect(detail.status).toBe("queued");
    expect(detail.error_diagnostic).toBeNull();
    expect(detail.tool_calls).toEqual([]);
    expect(detail.events).toBeUndefined();
  });

  it("reads one run with its transcript", async () => {
    const client = await sharedSignedIn();
    const page = await listRuns(client, { workspaceId: await seededWorkspaceId(client) });
    const seeded = page.results.find((run) => run.prompt === "contract-seed-run");
    expect(seeded).toBeDefined();
    const detail = await getRun(client, seeded?.id ?? "", { includeEvents: true });
    expect(detail.events?.map((event) => event.seq)).toEqual([1, 2]);
    expect(detail.events?.map((event) => event.kind)).toEqual(["run_started", "agent_narrative"]);
  });

  it("creates and cancels a free-form run", async () => {
    const client = await sharedSignedIn();
    const workspaceId = await seededWorkspaceId(client);
    const created = await createRun(client, {
      workspace: workspaceId,
      prompt: `Contract probe run ${Date.now()}`,
    });
    expect(created.status).toBe("queued");
    expect(created.pod_detail.project_identifier).toBe(CONTRACT_PROJECT_IDENTIFIER);
    const cancelled = await cancelRun(client, created.id, "contract probe cancel");
    expect(cancelled.status).toBe("cancelled");
    expect(cancelled.cancel_reason).toBe("contract probe cancel");
  });

  it("cancels the seeded probe run", async () => {
    const client = await sharedSignedIn();
    const page = await listRuns(client, { workspaceId: await seededWorkspaceId(client) });
    const probe = page.results.find(
      (run) =>
        run.prompt === "contract-seed-run-cancel" &&
        !["blocked", "completed", "failed", "cancelled", "refused"].includes(run.status)
    );
    expect(probe).toBeDefined();
    const cancelled = await cancelRun(client, probe?.id ?? "");
    expect(cancelled.status).toBe("cancelled");
  });

  it("reports unknown runs as not-found", async () => {
    const client = await sharedSignedIn();
    const failure = await getRun(client, "00000000-0000-0000-0000-000000000000").catch((error: unknown) => error);
    expect(failure).toBeInstanceOf(ApiError);
    expect((failure as ApiError).status).toBe(404);
  });

  it("rejects dispatch for unknown issues", async () => {
    const client = await sharedSignedIn();
    const workspaceId = await seededWorkspaceId(client);
    const missing = "00000000-0000-0000-0000-000000000000";
    for (const dispatch of [runAi, commentAndRun]) {
      const failure = await dispatch(client, { workspace: workspaceId, work_item: missing }).catch(
        (error: unknown) => error
      );
      expect(failure).toBeInstanceOf(ApiError);
      expect((failure as ApiError).status).toBe(404);
    }
  });

  it("answers re-tick on a fresh issue as a refusal", async () => {
    const client = await sharedSignedIn();
    const answer = await reTickIssue(client, { work_item: await seededIssueId(client) });
    expect(answer.granted).toBe(false);
    expect(typeof answer.reason).toBe("string");
    expect(answer.run_id).toBeNull();
  });

  it("reports re-tick on unknown issues as not-found", async () => {
    const client = await sharedSignedIn();
    const failure = await reTickIssue(client, {
      work_item: "00000000-0000-0000-0000-000000000000",
    }).catch((error: unknown) => error);
    expect(failure).toBeInstanceOf(ApiError);
    expect((failure as ApiError).status).toBe(404);
  });
});
