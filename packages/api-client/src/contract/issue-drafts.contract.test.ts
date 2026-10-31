// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: draft issues list, create, update, delete and
// promote-to-project against live Django.
import { describe, expect, it } from "vitest";
import { deleteIssue } from "../contracts/issue-actions.js";
import {
  createDraftIssue,
  deleteDraftIssue,
  getDraftIssue,
  listDraftIssues,
  promoteDraftIssue,
  updateDraftIssue,
} from "../contracts/issue-drafts.js";
import { listIssues } from "../contracts/issues.js";
import { listProjects } from "../contracts/projects.js";
import { CONTRACT_PROJECT_IDENTIFIER, CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";
import type { ApiClient } from "../client.js";

const DRAFT_NAME = "Contract draft probe";
const PROMOTE_NAME = "Contract draft promote probe";

async function seededProjectId(client: ApiClient): Promise<string> {
  const projects = await listProjects(client, CONTRACT_WORKSPACE);
  const seeded = projects.find((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER);
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

async function clearDrafts(client: ApiClient, names: string[]): Promise<void> {
  const page = await listDraftIssues(client, CONTRACT_WORKSPACE);
  for (const row of page.results.filter((entry) => entry.name !== null && names.includes(entry.name))) {
    await deleteDraftIssue(client, CONTRACT_WORKSPACE, row.id);
  }
}

async function clearPromoted(client: ApiClient, projectId: string): Promise<void> {
  const page = await listIssues(client, CONTRACT_WORKSPACE, projectId);
  for (const row of page.results.filter((entry) => entry.name === PROMOTE_NAME)) {
    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, row.id);
  }
}

describe.skipIf(!contractEnabled)("draft issues contract", () => {
  it("creates, reads, lists, updates and deletes a draft", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    await clearDrafts(client, [DRAFT_NAME]);

    const created = await createDraftIssue(client, CONTRACT_WORKSPACE, {
      name: DRAFT_NAME,
      project_id: projectId,
    });
    expect(created.name).toBe(DRAFT_NAME);
    expect(created.project_id).toBe(projectId);

    const reread = await getDraftIssue(client, CONTRACT_WORKSPACE, created.id);
    expect(reread.id).toBe(created.id);

    const listed = await listDraftIssues(client, CONTRACT_WORKSPACE);
    expect(listed.total_count).toBeGreaterThanOrEqual(1);
    expect(listed.results.map((row) => row.id)).toContain(created.id);

    await updateDraftIssue(client, CONTRACT_WORKSPACE, created.id, { name: `${DRAFT_NAME} v2` });
    const renamed = await getDraftIssue(client, CONTRACT_WORKSPACE, created.id);
    expect(renamed.name).toBe(`${DRAFT_NAME} v2`);

    await deleteDraftIssue(client, CONTRACT_WORKSPACE, created.id);
    const cleared = await listDraftIssues(client, CONTRACT_WORKSPACE);
    expect(cleared.results.map((row) => row.id)).not.toContain(created.id);
  });

  it("promotes a draft into a project issue", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    await clearDrafts(client, [PROMOTE_NAME]);
    await clearPromoted(client, projectId);

    const draft = await createDraftIssue(client, CONTRACT_WORKSPACE, {
      name: PROMOTE_NAME,
      project_id: projectId,
    });
    const promoted = await promoteDraftIssue(client, CONTRACT_WORKSPACE, draft.id, { name: PROMOTE_NAME });
    expect(promoted.name).toBe(PROMOTE_NAME);
    expect(promoted.project_id).toBe(projectId);

    const drafts = await listDraftIssues(client, CONTRACT_WORKSPACE);
    expect(drafts.results.map((row) => row.id)).not.toContain(draft.id);

    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, promoted.id);
  });
});
