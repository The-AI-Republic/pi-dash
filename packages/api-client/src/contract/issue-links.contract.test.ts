// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: external links list, add, edit and remove against live Django.
import { describe, expect, it } from "vitest";
import { createIssue, deleteIssue } from "../contracts/issue-actions.js";
import { createIssueLink, deleteIssueLink, listIssueLinks, updateIssueLink } from "../contracts/issue-links.js";
import { listIssues } from "../contracts/issues.js";
import { listProjects } from "../contracts/projects.js";
import { CONTRACT_PROJECT_IDENTIFIER, CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";
import type { ApiClient } from "../client.js";

const PROBE_NAME = "Contract link probe";

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

describe.skipIf(!contractEnabled)("issue links contract", () => {
  it("adds, edits, lists and removes a link", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const probeId = await freshProbe(client, projectId);

    const created = await createIssueLink(client, CONTRACT_WORKSPACE, projectId, probeId, {
      url: "https://contract.example/spec",
      title: "Contract spec",
    });
    expect(created.url).toBe("https://contract.example/spec");
    expect(created.title).toBe("Contract spec");
    expect(created.issue).toBe(probeId);

    const listed = await listIssueLinks(client, CONTRACT_WORKSPACE, projectId, probeId);
    expect(listed.map((row) => row.id)).toContain(created.id);

    const renamed = await updateIssueLink(client, CONTRACT_WORKSPACE, projectId, probeId, created.id, {
      title: "Contract spec v2",
    });
    expect(renamed.title).toBe("Contract spec v2");
    expect(renamed.url).toBe("https://contract.example/spec");

    await deleteIssueLink(client, CONTRACT_WORKSPACE, projectId, probeId, created.id);
    const cleared = await listIssueLinks(client, CONTRACT_WORKSPACE, projectId, probeId);
    expect(cleared.map((row) => row.id)).not.toContain(created.id);

    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, probeId);
  });

  it("prefixes a missing URL scheme", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const probeId = await freshProbe(client, projectId);

    const created = await createIssueLink(client, CONTRACT_WORKSPACE, projectId, probeId, {
      url: "contract.example/bare",
    });
    expect(created.url).toBe("http://contract.example/bare");

    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, probeId);
  });
});
