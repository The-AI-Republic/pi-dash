// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: project estimate writes against live Django — create a system
// with its points, read it back through the list, delete it, and confirm
// the 204. The seeded estimate proves the read shape on a system the seed
// created, not one the test made.
import { describe, expect, it } from "vitest";
import { ApiError } from "../errors.js";
import { createEstimate, deleteEstimate, listProjectEstimates, selectActiveEstimate } from "../contracts/estimates.js";
import { getProject, listProjects } from "../contracts/projects.js";
import { updateProject } from "../contracts/project-management.js";
import { CONTRACT_PROJECT_IDENTIFIER, CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";
import type { ApiClient } from "../client.js";

async function signedIn(): Promise<ApiClient> {
  return sharedSignedIn();
}

async function seededProjectId(client: ApiClient): Promise<string> {
  const projects = await listProjects(client, CONTRACT_WORKSPACE);
  const seeded = projects.find((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER);
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

describe.skipIf(!contractEnabled)("project estimate write contract", () => {
  it("reads the seeded system with its points and active marker", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const estimates = await listProjectEstimates(client, CONTRACT_WORKSPACE, projectId);
    const seeded = estimates.find((estimate) => estimate.name === "Contract estimate");
    expect(seeded).toBeDefined();
    expect(seeded?.points.length).toBeGreaterThan(0);
    expect(seeded?.points[0]?.key).toBeGreaterThan(0);
    expect(seeded?.points[0]?.description).toBeTypeOf("string");
    expect(seeded?.workspace.length).toBeGreaterThan(0);
    expect(seeded?.project).toBe(projectId);
    expect(selectActiveEstimate(estimates)).toBeDefined();
  });

  it("creates a system with points, lists it, then deletes it", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const name = `Contract write ${Date.now().toString(36)}`;
    const created = await createEstimate(client, CONTRACT_WORKSPACE, projectId, {
      name,
      type: "categories",
      last_used: false,
      points: [
        { key: 1, value: "Small" },
        { key: 2, value: "Large", description: "Large effort" },
      ],
    });
    expect(created.name).toBe(name);
    expect(created.type).toBe("categories");
    expect(created.last_used).toBe(false);
    expect(created.project).toBe(projectId);
    expect(created.points.map((point) => point.value).sort()).toEqual(["Large", "Small"]);
    expect(created.points.find((point) => point.value === "Large")?.description).toBe("Large effort");
    try {
      const listed = await listProjectEstimates(client, CONTRACT_WORKSPACE, projectId);
      expect(listed.map((estimate) => estimate.id)).toContain(created.id);
    } finally {
      await expect(deleteEstimate(client, CONTRACT_WORKSPACE, projectId, created.id)).resolves.toBe(204);
    }
    const after = await listProjectEstimates(client, CONTRACT_WORKSPACE, projectId);
    expect(after.map((estimate) => estimate.id)).not.toContain(created.id);
  });

  it("defaults the type and points when the caller omits them", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const name = `Contract bare ${Date.now().toString(36)}`;
    const created = await createEstimate(client, CONTRACT_WORKSPACE, projectId, { name });
    try {
      expect(created.name).toBe(name);
      expect(created.type).toBe("categories");
      expect(created.points).toEqual([]);
    } finally {
      await deleteEstimate(client, CONTRACT_WORKSPACE, projectId, created.id).catch(() => 0);
    }
  });

  it("sets and clears the project's active estimate through the project update", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const name = `Contract switch ${Date.now().toString(36)}`;
    const created = await createEstimate(client, CONTRACT_WORKSPACE, projectId, { name, type: "points" });
    try {
      const enabled = await updateProject(client, CONTRACT_WORKSPACE, projectId, { estimate: created.id });
      expect(enabled.estimate).toBe(created.id);
      const readBack = await getProject(client, CONTRACT_WORKSPACE, projectId);
      expect(readBack.estimate).toBe(created.id);
      const cleared = await updateProject(client, CONTRACT_WORKSPACE, projectId, { estimate: null });
      expect(cleared.estimate).toBeNull();
    } finally {
      await updateProject(client, CONTRACT_WORKSPACE, projectId, { estimate: null }).catch(() => 0);
      await deleteEstimate(client, CONTRACT_WORKSPACE, projectId, created.id).catch(() => 0);
    }
  });

  it("404s a delete for an unknown system", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const gone = await deleteEstimate(
      client,
      CONTRACT_WORKSPACE,
      projectId,
      "00000000-0000-4000-8000-000000000000"
    ).then(
      () => null,
      (caught: unknown) => caught
    );
    expect(gone).toBeInstanceOf(ApiError);
    expect((gone as ApiError).status).toBe(404);
  });
});
