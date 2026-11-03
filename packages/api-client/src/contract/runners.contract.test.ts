// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: runner list/get/revoke/delete against live Django. The
// revoke and delete tests consume seed probes; re-run
// contract/seed-contracts.py before re-running this suite.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import { ApiError } from "../errors.js";
import { listProjects } from "../contracts/projects.js";
import { deleteRunner, getRunner, listRunners, revokeRunner } from "../contracts/runners.js";
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

describe.skipIf(!contractEnabled)("runners contract", () => {
  it("lists workspace runners", async () => {
    const client = await sharedSignedIn();
    const runners = await listRunners(client, await seededWorkspaceId(client));
    const keeper = runners.find((runner) => runner.name === "contract-runner");
    expect(keeper).toBeDefined();
    expect(keeper?.pod_detail.project_identifier).toBe(CONTRACT_PROJECT_IDENTIFIER);
  });

  it("scopes the list to one project", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const runners = await listRunners(client, await seededWorkspaceId(client), { projectId });
    expect(runners.length).toBeGreaterThan(0);
    expect(runners.every((runner) => runner.pod_detail.project === projectId)).toBe(true);
  });

  it("reads one runner with its nested shapes", async () => {
    const client = await sharedSignedIn();
    const runners = await listRunners(client, await seededWorkspaceId(client));
    const keeper = runners.find((runner) => runner.name === "contract-runner");
    expect(keeper).toBeDefined();
    const detail = await getRunner(client, keeper?.id ?? "");
    expect(detail.name).toBe("contract-runner");
    expect(detail.status).toBe("offline");
    expect(detail.dev_machine_detail?.label).toBe("contract-machine");
    expect(detail.live_state).toBeNull();
    expect(detail.enrolled_at).toBeNull();
  });

  it("revokes a runner and keeps the row", async () => {
    const client = await sharedSignedIn();
    const runners = await listRunners(client, await seededWorkspaceId(client));
    const probe = runners.find((runner) => runner.name === "contract-runner-revoke");
    expect(probe).toBeDefined();
    const revoked = await revokeRunner(client, probe?.id ?? "");
    expect(revoked.status).toBe("revoked");
    expect(revoked.revoked_at).not.toBeNull();
  });

  it("deletes a runner and drops the row", async () => {
    const client = await sharedSignedIn();
    const runners = await listRunners(client, await seededWorkspaceId(client));
    const probe = runners.find((runner) => runner.name === "contract-runner-delete");
    expect(probe).toBeDefined();
    const status = await deleteRunner(client, probe?.id ?? "");
    expect(status).toBe(204);
    const gone = await getRunner(client, probe?.id ?? "").catch((error: unknown) => error);
    expect(gone).toBeInstanceOf(ApiError);
    expect((gone as ApiError).status).toBe(404);
  });

  it("reports unknown runners as not-found", async () => {
    const client = await sharedSignedIn();
    const failure = await getRunner(client, "00000000-0000-0000-0000-000000000000").catch((error: unknown) => error);
    expect(failure).toBeInstanceOf(ApiError);
    expect((failure as ApiError).status).toBe(404);
  });
});
