// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: project estimate writes against live Django — create a system
// with its points, read it back through the list, delete it, and confirm
// the 204. The seeded estimate proves the read shape on a system the seed
// created, not one the test made.
import { beforeAll, describe, expect, it } from "vitest";
import { ApiError } from "../errors.js";
import {
  createEstimate,
  deleteEstimate,
  getProjectEstimate,
  listProjectEstimatePoints,
  listProjectEstimates,
  orderEstimatePoints,
  selectActiveEstimate,
} from "../contracts/estimates.js";
import { getProject, listProjects } from "../contracts/projects.js";
import { updateProject } from "../contracts/project-management.js";
import { signIn } from "../contracts/auth-entry.js";
import {
  CONTRACT_PASSWORD,
  CONTRACT_PROJECT_IDENTIFIER,
  CONTRACT_SECONDARY_EMAIL,
  CONTRACT_WORKSPACE,
  contractClient,
  contractEnabled,
  sharedSignedIn,
} from "./setup.js";
import type { ApiClient } from "../client.js";

async function signedIn(): Promise<ApiClient> {
  return sharedSignedIn();
}

async function projectIdFor(client: ApiClient, identifier: string): Promise<string> {
  const projects = await listProjects(client, CONTRACT_WORKSPACE);
  const seeded = projects.find((project) => project.identifier === identifier);
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

async function seededProjectId(client: ApiClient): Promise<string> {
  return projectIdFor(client, CONTRACT_PROJECT_IDENTIFIER);
}

/** The second seeded project: no estimate systems, no membership for the probe. */
async function bareProjectId(client: ApiClient): Promise<string> {
  return projectIdFor(client, "CU");
}

function uniqueName(prefix: string): string {
  return `${prefix} ${Date.now().toString(36)}${Math.floor(Math.random() * 1e4).toString(36)}`;
}

/**
 * Two refusal fixtures: the probe is a workspace admin (so it can still
 * update a project it has no membership on), while the membership user is a
 * plain workspace member, which the project update refuses.
 */
const NON_MEMBER_EMAIL = process.env["PIDASH_CONTRACT_MEMBERSHIP_EMAIL"] ?? "contract.membership@example.com";

let probe: ApiClient;
let nonMember: ApiClient;

beforeAll(async () => {
  if (!contractEnabled) return;
  probe = contractClient();
  const probeSignIn = await signIn(probe, { email: CONTRACT_SECONDARY_EMAIL, password: CONTRACT_PASSWORD });
  expect(probeSignIn.ok).toBe(true);
  nonMember = contractClient();
  const memberSignIn = await signIn(nonMember, { email: NON_MEMBER_EMAIL, password: CONTRACT_PASSWORD });
  expect(memberSignIn.ok).toBe(true);
});

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

describe.skipIf(!contractEnabled)("estimate system reads", () => {
  it("reads a created system by id through the project-scoped single read", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const created = await createEstimate(client, CONTRACT_WORKSPACE, projectId, {
      name: uniqueName("Contract single"),
      type: "points",
      points: [
        { key: 1, value: "2" },
        { key: 2, value: "1" },
      ],
    });
    try {
      const read = await getProjectEstimate(client, CONTRACT_WORKSPACE, projectId, created.id);
      expect(read.id).toBe(created.id);
      expect(read.name).toBe(created.name);
      expect(read.points).toHaveLength(2);
    } finally {
      await deleteEstimate(client, CONTRACT_WORKSPACE, projectId, created.id).catch(() => 0);
    }
  });

  it("404s a single read for an unknown system", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const failure = await getProjectEstimate(
      client,
      CONTRACT_WORKSPACE,
      projectId,
      "00000000-0000-4000-8000-000000000000"
    ).then(
      () => null,
      (caught: unknown) => caught
    );
    expect(failure).toBeInstanceOf(ApiError);
    expect((failure as ApiError).status).toBe(404);
    expect((failure as ApiError).message).toBe("The required object does not exist.");
  });

  it("lists no systems for a project that has none", async () => {
    const client = await signedIn();
    const projectId = await bareProjectId(client);
    await expect(listProjectEstimates(client, CONTRACT_WORKSPACE, projectId)).resolves.toEqual([]);
  });

  it("round-trips the caller's 1..n positions through create, list and single read", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const created = await createEstimate(client, CONTRACT_WORKSPACE, projectId, {
      name: uniqueName("Contract order"),
      type: "points",
      last_used: false,
      points: [
        { key: 1, value: "2" },
        { key: 2, value: "1" },
        { key: 3, value: "3" },
      ],
    });
    try {
      expect(orderEstimatePoints(created.points).map((entry) => entry.value)).toEqual(["2", "1", "3"]);
      const listed = (await listProjectEstimates(client, CONTRACT_WORKSPACE, projectId)).find(
        (estimate) => estimate.id === created.id
      );
      expect(listed).toBeDefined();
      const listedOrdered = orderEstimatePoints(listed?.points ?? []);
      expect(listedOrdered.map((entry) => entry.key)).toEqual([1, 2, 3]);
      expect(listedOrdered.map((entry) => entry.value)).toEqual(["2", "1", "3"]);
      const read = await getProjectEstimate(client, CONTRACT_WORKSPACE, projectId, created.id);
      expect(orderEstimatePoints(read.points).map((entry) => entry.value)).toEqual(["2", "1", "3"]);
    } finally {
      await deleteEstimate(client, CONTRACT_WORKSPACE, projectId, created.id).catch(() => 0);
    }
  });

  it("generates a lowercase name when the body omits one", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const created = await createEstimate(client, CONTRACT_WORKSPACE, projectId, { type: "categories" });
    try {
      expect(created.name).toMatch(/^[a-z]{10}$/);
      expect(created.type).toBe("categories");
      expect(selectActiveEstimate([{ ...created, last_used: true }])?.id).toBe(created.id);
    } finally {
      await deleteEstimate(client, CONTRACT_WORKSPACE, projectId, created.id).catch(() => 0);
    }
  });

  it("stores the raw server default 0 for a row sent without a key", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const name = uniqueName("Contract raw key");
    const path = `/api/workspaces/${CONTRACT_WORKSPACE}/projects/${projectId}/estimates/`;
    const envelope = await client.sendJson<{ id: string }>("POST", path, {
      json: {
        estimate: { name, type: "points" },
        estimate_points: [{ key: 1, value: "1" }, { value: "2" }],
      },
    });
    try {
      const read = await getProjectEstimate(client, CONTRACT_WORKSPACE, projectId, envelope.data.id);
      const missingKey = read.points.find((entry) => entry.value === "2");
      // The server default is a raw column value, never a display position.
      expect(missingKey?.key).toBe(0);
      expect(orderEstimatePoints(read.points).map((entry) => entry.value)).toEqual(["2", "1"]);
    } finally {
      await deleteEstimate(client, CONTRACT_WORKSPACE, projectId, envelope.data.id).catch(() => 0);
    }
  });

  it("reads the linked project's values through the project-points endpoint", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const original = (await getProject(client, CONTRACT_WORKSPACE, projectId)).estimate;
    const created = await createEstimate(client, CONTRACT_WORKSPACE, projectId, {
      name: uniqueName("Contract project points"),
      type: "categories",
      points: [
        { key: 1, value: "Small" },
        { key: 2, value: "Large" },
      ],
    });
    try {
      await updateProject(client, CONTRACT_WORKSPACE, projectId, { estimate: created.id });
      const points = await listProjectEstimatePoints(client, CONTRACT_WORKSPACE, projectId);
      expect(orderEstimatePoints(points).map((entry) => entry.value)).toEqual(["Small", "Large"]);
    } finally {
      await updateProject(client, CONTRACT_WORKSPACE, projectId, { estimate: original ?? null }).catch(() => 0);
      await deleteEstimate(client, CONTRACT_WORKSPACE, projectId, created.id).catch(() => 0);
    }
  });

  it("answers an empty value list for a project with no linked system", async () => {
    const client = await signedIn();
    const projectId = await bareProjectId(client);
    await expect(listProjectEstimatePoints(client, CONTRACT_WORKSPACE, projectId)).resolves.toEqual([]);
  });
});

describe.skipIf(!contractEnabled)("estimate error shapes", () => {
  it("400s a create whose value exceeds the server's 20-character cap", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const failure = await createEstimate(client, CONTRACT_WORKSPACE, projectId, {
      name: uniqueName("Contract long value"),
      type: "points",
      points: [{ key: 1, value: "x".repeat(21) }],
    }).then(
      () => null,
      (caught: unknown) => caught
    );
    expect(failure).toBeInstanceOf(ApiError);
    expect((failure as ApiError).status).toBe(400);
    // many=True validate() errors come back as a per-row array, not a field map.
    expect((failure as ApiError).body).toEqual([{ non_field_errors: ["Value can't be more than 20 characters"] }]);
  });

  it("403s every estimate read and write for a project the caller does not belong to", async () => {
    const client = await signedIn();
    const projectId = await bareProjectId(client);
    const refused = async (call: Promise<unknown>): Promise<ApiError | null> =>
      call.then(
        () => null,
        (caught: unknown) => caught as ApiError
      );

    const list = await refused(listProjectEstimates(probe, CONTRACT_WORKSPACE, projectId));
    const single = await refused(
      getProjectEstimate(probe, CONTRACT_WORKSPACE, projectId, "00000000-0000-4000-8000-000000000000")
    );
    const points = await refused(listProjectEstimatePoints(probe, CONTRACT_WORKSPACE, projectId));
    const write = await refused(
      createEstimate(probe, CONTRACT_WORKSPACE, projectId, { name: uniqueName("Contract refused") })
    );
    for (const failure of [list, single, points, write]) {
      expect(failure).toBeInstanceOf(ApiError);
      expect(failure?.status).toBe(403);
    }
  });

  it("403s the project link write for a workspace member without project admin", async () => {
    const projectId = await bareProjectId(await signedIn());
    const failure = await updateProject(nonMember, CONTRACT_WORKSPACE, projectId, { estimate: null }).then(
      () => null,
      (caught: unknown) => caught
    );
    expect(failure).toBeInstanceOf(ApiError);
    expect((failure as ApiError).status).toBe(403);
    expect((failure as ApiError).message).toBe("You don't have the required permissions.");
  });
});
