// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: the agent-status read the issue detail carries, against
// live Django. Run-level writes (dispatch, cancel, re-tick) belong
// to the runners area and prove out in its own suites.
import { describe, expect, it } from "vitest";
import { getIssue, listIssues } from "../contracts/issues.js";
import { listProjects } from "../contracts/projects.js";
import { CONTRACT_PROJECT_IDENTIFIER, CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";
import type { ApiClient } from "../client.js";

const AGENT_PROBE = "Contract agent probe";

async function seededProjectId(client: ApiClient): Promise<string> {
  const projects = await listProjects(client, CONTRACT_WORKSPACE);
  const seeded = projects.find((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER);
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

async function issueIdByName(client: ApiClient, projectId: string, name: string): Promise<string> {
  const page = await listIssues(client, CONTRACT_WORKSPACE, projectId);
  const row = page.results.find((entry) => entry.name === name);
  expect(row).toBeDefined();
  return row?.id ?? "";
}

describe.skipIf(!contractEnabled)("issue agent contract", () => {
  it("reads the agent status with its token figures", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const detail = await getIssue(
      client,
      CONTRACT_WORKSPACE,
      projectId,
      await issueIdByName(client, projectId, AGENT_PROBE)
    );

    expect(detail.agent_ticker?.used).toBe(0);
    expect(detail.agent_ticker?.max_ticks).toBe(10);
    expect(detail.agent_ticker?.can_re_tick).toBe(false);
    expect(detail.agent_status?.run_count).toBe(1);
    expect(detail.agent_status?.active_run).toBeNull();
    expect(detail.agent_status?.latest_run?.status).toBe("completed");
    expect(detail.agent_status?.latest_run?.total_tokens).toBe(1540);
    expect(detail.agent_status?.ticker?.used).toBe(0);
  });

  it("reads null agent state on an issue without runs", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const detail = await getIssue(
      client,
      CONTRACT_WORKSPACE,
      projectId,
      await issueIdByName(client, projectId, "Contract issue one")
    );
    expect(detail.agent_status).toBeNull();
    expect(detail.agent_ticker).toBeNull();
  });
});
