// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: pod list/get/create/update/delete against live Django.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import { ApiError } from "../errors.js";
import { createPod, deletePod, getPod, listPods, updatePod } from "../contracts/pods.js";
import { listProjects } from "../contracts/projects.js";
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

let podNonce = 0;
function podSuffix(): string {
  podNonce += 1;
  return `contract-${Date.now().toString(36)}-${podNonce}`;
}

describe.skipIf(!contractEnabled)("pods contract", () => {
  it("lists workspace pods default-first", async () => {
    const client = await sharedSignedIn();
    const pods = await listPods(client, { workspaceId: await seededWorkspaceId(client) });
    expect(pods.length).toBeGreaterThan(0);
    expect(pods[0]?.is_default).toBe(true);
    expect(pods.some((pod) => pod.project_identifier === CONTRACT_PROJECT_IDENTIFIER)).toBe(true);
  });

  it("lists one project's pods", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const pods = await listPods(client, { projectId });
    expect(pods.length).toBeGreaterThan(0);
    expect(pods.every((pod) => pod.project === projectId)).toBe(true);
  });

  it("prefers the project filter over the workspace one", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const pods = await listPods(client, { projectId, workspaceId: await seededWorkspaceId(client) });
    expect(pods.length).toBeGreaterThan(0);
    expect(pods.every((pod) => pod.project === projectId)).toBe(true);
  });

  it("creates, reads, renames, promotes, demotes and deletes a pod", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const before = await listPods(client, { projectId });
    const originalDefault = before.find((pod) => pod.is_default);
    expect(originalDefault).toBeDefined();

    const created = await createPod(client, { project: projectId, name: podSuffix() });
    expect(created.name.startsWith(`${CONTRACT_PROJECT_IDENTIFIER}_contract-`)).toBe(true);
    expect(created.is_default).toBe(false);

    const read = await getPod(client, created.id);
    expect(read.id).toBe(created.id);
    expect(read.description).toBe("");

    const renamed = await updatePod(client, created.id, {
      name: podSuffix(),
      description: "Contract probe pod",
    });
    expect(renamed.description).toBe("Contract probe pod");
    expect(renamed.name.startsWith(`${CONTRACT_PROJECT_IDENTIFIER}_contract-`)).toBe(true);

    const promoted = await updatePod(client, created.id, { is_default: true });
    expect(promoted.is_default).toBe(true);

    // Hand the flag back so the seed project ends as it started.
    await updatePod(client, originalDefault?.id ?? "", { is_default: true });
    const demoted = await getPod(client, created.id);
    expect(demoted.is_default).toBe(false);

    const status = await deletePod(client, created.id);
    expect(status).toBe(204);
    const gone = await getPod(client, created.id).catch((error: unknown) => error);
    expect(gone).toBeInstanceOf(ApiError);
    expect((gone as ApiError).status).toBe(404);
  });

  it("reports unknown pods as not-found", async () => {
    const client = await sharedSignedIn();
    const failure = await getPod(client, "00000000-0000-0000-0000-000000000000").catch((error: unknown) => error);
    expect(failure).toBeInstanceOf(ApiError);
    expect((failure as ApiError).status).toBe(404);
  });
});
