// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: project-settings member and workflow-state writes against live
// Django. Probe memberships and states are removed by contract; the seeded
// membership fixture is a workspace member with no active project row.
import { beforeEach, describe, expect, it } from "vitest";
import {
  addProjectMembers,
  createState,
  deleteState,
  markStateDefault,
  removeProjectMember,
  updateProjectMember,
  updateState,
} from "../contracts/project-settings-writes.js";
import { listMembers, listStates } from "../contracts/reference.js";
import { listProjects } from "../contracts/projects.js";
import { listWorkspaceRoster } from "../contracts/workspace-management.js";
import { CONTRACT_PROJECT_IDENTIFIER, CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";
import type { ApiClient } from "../client.js";

const MEMBER_EMAIL = process.env["PIDASH_CONTRACT_MEMBERSHIP_EMAIL"] ?? "contract.membership@example.com";
const PROBE_STATE_PREFIX = "Contract probe state";
const PROBE_STATE_NAME = `${PROBE_STATE_PREFIX} ${Date.now().toString(36)}`;

async function seededProjectId(client: ApiClient): Promise<string> {
  const projects = await listProjects(client, CONTRACT_WORKSPACE);
  const seeded = projects.find((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER);
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

/**
 * Resolve the fixture user id through the workspace directory. The member
 * list read answers user ids, not membership ids, so resolve the roster
 * membership separately for the remove call.
 */
async function fixtureMemberId(client: ApiClient): Promise<{ userId: string }> {
  const roster = await listWorkspaceRoster(client, CONTRACT_WORKSPACE);
  const found = roster.find((row) => row.member.email === MEMBER_EMAIL);
  expect(found).toBeDefined();
  return { userId: found?.member.id ?? "" };
}

async function deactivateFixture(client: ApiClient, projectId: string, userId: string): Promise<void> {
  const memberships = await listMembers(client, CONTRACT_WORKSPACE, projectId);
  for (const row of memberships.filter((entry) => entry.member === userId)) {
    await removeProjectMember(client, CONTRACT_WORKSPACE, projectId, row.id).catch(() => 0);
  }
}

async function ensureDefault(client: ApiClient, projectId: string): Promise<string> {
  const states = await listStates(client, CONTRACT_WORKSPACE, projectId);
  const defaultState = states.find((state) => state.default);
  if (defaultState) return defaultState.id;
  const fallback = states.find((state) => state.group === "backlog") ?? states[0];
  expect(fallback).toBeDefined();
  await markStateDefault(client, CONTRACT_WORKSPACE, projectId, fallback?.id ?? "");
  return fallback?.id ?? "";
}

async function clearProbeStates(client: ApiClient, projectId: string): Promise<void> {
  const states = await listStates(client, CONTRACT_WORKSPACE, projectId);
  for (const state of states) {
    if (state.name.startsWith(PROBE_STATE_PREFIX)) {
      await deleteState(client, CONTRACT_WORKSPACE, projectId, state.id).catch(() => 0);
    }
  }
}

let client: ApiClient;
let projectId: string;
let fixture: { userId: string };

beforeEach(async () => {
  client = await sharedSignedIn();
  projectId = await seededProjectId(client);
  fixture = await fixtureMemberId(client);
});

describe.skipIf(!contractEnabled)("project member writes contract", () => {
  it("adds, re-roles and removes a workspace member", async () => {
    await deactivateFixture(client, projectId, fixture.userId);

    const added = await addProjectMembers(client, CONTRACT_WORKSPACE, projectId, [
      { member_id: fixture.userId, role: 15 },
    ]);
    const membership = added.find((row) => row.member === fixture.userId);
    expect(membership).toBeDefined();
    expect(membership?.role).toBe(15);
    expect(membership?.project).toBe(projectId);
    expect(membership?.original_role).toBe(15);

    const listed = await listMembers(client, CONTRACT_WORKSPACE, projectId);
    expect(listed.some((row) => row.id === membership?.id && row.member === fixture.userId)).toBe(true);

    const updated = await updateProjectMember(client, CONTRACT_WORKSPACE, projectId, membership?.id ?? "", 5);
    expect(updated.role).toBe(5);
    expect(updated.member.id).toBe(fixture.userId);
    expect(updated.project.id).toBe(projectId);
    expect(updated.workspace.slug).toBe(CONTRACT_WORKSPACE);
    expect(updated.is_active).toBe(true);

    await expect(removeProjectMember(client, CONTRACT_WORKSPACE, projectId, membership?.id ?? "")).resolves.toBe(204);
    const afterRemove = await listMembers(client, CONTRACT_WORKSPACE, projectId);
    expect(afterRemove.some((row) => row.member === fixture.userId)).toBe(false);
  });

  it("refuses a bulk add with no members", async () => {
    const failure = await addProjectMembers(client, CONTRACT_WORKSPACE, projectId, []).catch((error: unknown) => error);
    expect(failure).toBeInstanceOf(Error);
    expect((failure as { status?: number }).status).toBe(400);
  });
});

describe.skipIf(!contractEnabled)("workflow state writes contract", () => {
  it("creates, renames, recolors, regroups, reorders, marks default and deletes a state", async () => {
    await clearProbeStates(client, projectId);
    const { state: created, status: createStatus } = await createState(client, CONTRACT_WORKSPACE, projectId, {
      name: PROBE_STATE_NAME,
      color: "#123456",
      group: "started",
      description: "Contract probe state.",
    });
    expect(createStatus).toBe(200);
    expect(created.name).toBe(PROBE_STATE_NAME);
    expect(created.color).toBe("#123456");
    expect(created.group).toBe("started");
    expect(created.description).toBe("Contract probe state.");
    expect(created.project_id).toBe(projectId);
    expect(created.default).toBe(false);

    try {
      const updated = await updateState(client, CONTRACT_WORKSPACE, projectId, created.id, {
        name: `${PROBE_STATE_NAME} renamed`,
        color: "#654321",
        group: "review",
        description: "Renamed.",
        sequence: 777,
      });
      expect(updated.id).toBe(created.id);
      expect(updated.name).toBe(`${PROBE_STATE_NAME} renamed`);
      expect(updated.color).toBe("#654321");
      expect(updated.group).toBe("review");
      expect(updated.description).toBe("Renamed.");
      expect(updated.sequence).toBe(777);

      const previousDefaultId = await ensureDefault(client, projectId);
      await expect(markStateDefault(client, CONTRACT_WORKSPACE, projectId, created.id)).resolves.toBe(204);
      const defaults = (await listStates(client, CONTRACT_WORKSPACE, projectId)).filter((state) => state.default);
      expect(defaults.map((state) => state.id)).toEqual([created.id]);

      // The probe is the default now, so the server refuses deletion; move
      // the default back first, then delete the plain state.
      const refused = await deleteState(client, CONTRACT_WORKSPACE, projectId, created.id).catch(
        (error: unknown) => error
      );
      expect(refused).toBeInstanceOf(Error);
      expect((refused as { status?: number }).status).toBe(400);

      await markStateDefault(client, CONTRACT_WORKSPACE, projectId, previousDefaultId);
      await expect(deleteState(client, CONTRACT_WORKSPACE, projectId, created.id)).resolves.toBe(204);
      const afterDelete = await listStates(client, CONTRACT_WORKSPACE, projectId);
      expect(afterDelete.some((state) => state.id === created.id)).toBe(false);
    } finally {
      await clearProbeStates(client, projectId);
    }
  });

  it("refuses a duplicate state name and the triage group", async () => {
    await clearProbeStates(client, projectId);
    const { state: duplicate } = await createState(client, CONTRACT_WORKSPACE, projectId, {
      name: PROBE_STATE_NAME,
      color: "#123456",
      group: "backlog",
    });
    try {
      const duplicateFailure = await createState(client, CONTRACT_WORKSPACE, projectId, {
        name: PROBE_STATE_NAME,
        color: "#123456",
        group: "backlog",
      }).catch((error: unknown) => error);
      expect(duplicateFailure).toBeInstanceOf(Error);
      expect((duplicateFailure as { status?: number }).status).toBe(400);

      const triageFailure = await createState(client, CONTRACT_WORKSPACE, projectId, {
        name: `${PROBE_STATE_NAME} triage`,
        color: "#123456",
        group: "triage",
      }).catch((error: unknown) => error);
      expect(triageFailure).toBeInstanceOf(Error);
      expect((triageFailure as { status?: number }).status).toBe(400);
    } finally {
      await deleteState(client, CONTRACT_WORKSPACE, projectId, duplicate.id).catch(() => 0);
      await clearProbeStates(client, projectId);
    }
  });

  it("refuses to delete the default state", async () => {
    const defaultStateId = await ensureDefault(client, projectId);
    const failure = await deleteState(client, CONTRACT_WORKSPACE, projectId, defaultStateId).catch(
      (error: unknown) => error
    );
    expect(failure).toBeInstanceOf(Error);
    expect((failure as { status?: number }).status).toBe(400);
  });
});
