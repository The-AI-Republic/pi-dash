// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: label writes plus the intake-state read against live Django.
// Probe labels are deleted by contract; the read-only intake state comes
// from the seeded triage row.
import { describe, expect, it } from "vitest";
import { createLabel, deleteLabel, getIntakeState, listLabels, updateLabel } from "../contracts/reference.js";
import { listProjects } from "../contracts/projects.js";
import { CONTRACT_PROJECT_IDENTIFIER, CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";
import type { ApiClient } from "../client.js";

const PROBE_PREFIX = "contract-probe-label";

async function seededProjectId(client: ApiClient): Promise<string> {
  const projects = await listProjects(client, CONTRACT_WORKSPACE);
  const seeded = projects.find((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER);
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

/** Remove probe rows a crashed run may have left behind. */
async function clearProbeLabels(client: ApiClient, projectId: string): Promise<void> {
  const labels = await listLabels(client, CONTRACT_WORKSPACE, projectId);
  for (const label of labels) {
    if (label.name === PROBE_PREFIX || label.name.startsWith(`${PROBE_PREFIX}-`)) {
      await deleteLabel(client, CONTRACT_WORKSPACE, projectId, label.id);
    }
  }
}

describe.skipIf(!contractEnabled)("issue label contract", () => {
  it("creates, renames, recolors, groups, reorders and deletes", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    await clearProbeLabels(client, projectId);

    const created = await createLabel(client, CONTRACT_WORKSPACE, projectId, {
      name: PROBE_PREFIX,
      color: "#ff0000",
    });
    expect(created.name).toBe(PROBE_PREFIX);
    expect(created.color).toBe("#ff0000");
    expect(created.parent).toBeNull();
    // Fresh labels take large incremental sort orders from the server.
    expect(created.sort_order).toBeGreaterThan(0);
    const child = await createLabel(client, CONTRACT_WORKSPACE, projectId, {
      name: `${PROBE_PREFIX}-child`,
      color: "#00ff00",
    });
    try {
      const renamed = await updateLabel(client, CONTRACT_WORKSPACE, projectId, created.id, {
        name: `${PROBE_PREFIX}-renamed`,
        color: "#0000ff",
      });
      expect(renamed.name).toBe(`${PROBE_PREFIX}-renamed`);
      expect(renamed.color).toBe("#0000ff");

      const grouped = await updateLabel(client, CONTRACT_WORKSPACE, projectId, child.id, {
        parent: created.id,
        sort_order: 12.5,
      });
      expect(grouped.parent).toBe(created.id);
      expect(grouped.sort_order).toBe(12.5);

      const ungrouped = await updateLabel(client, CONTRACT_WORKSPACE, projectId, child.id, { parent: null });
      expect(ungrouped.parent).toBeNull();
    } finally {
      await deleteLabel(client, CONTRACT_WORKSPACE, projectId, child.id);
      await deleteLabel(client, CONTRACT_WORKSPACE, projectId, created.id);
    }
    const after = await listLabels(client, CONTRACT_WORKSPACE, projectId);
    expect(after.some((label) => label.id === created.id || label.id === child.id)).toBe(false);
  });
});

describe.skipIf(!contractEnabled)("intake state contract", () => {
  it("reads the project's triage state", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const state = await getIntakeState(client, CONTRACT_WORKSPACE, projectId);
    expect(state.group).toBe("triage");
    expect(state.project_id).toBe(projectId);
    expect(state.name).toBe("Triage");
  });
});
