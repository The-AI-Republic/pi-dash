// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: project estimate writes against live Django — create a system
// with its points, read it back through the list, delete it, and confirm
// the 204. The seeded estimate proves the read shape on a system the seed
// created, not one the test made. The second half covers the editing
// surface: the saved-system update, single-value create/update/delete,
// and the error matrix around them.
import { beforeAll, describe, expect, it } from "vitest";
import { ApiError } from "../errors.js";
import {
  createEstimate,
  createProjectEstimatePoint,
  deleteEstimate,
  deleteProjectEstimatePoint,
  getProjectEstimate,
  listProjectEstimatePoints,
  listProjectEstimates,
  orderEstimatePoints,
  selectActiveEstimate,
  updateProjectEstimate,
  updateProjectEstimatePoint,
} from "../contracts/estimates.js";
import { getIssue } from "../contracts/issues.js";
import { createIssue, deleteIssue } from "../contracts/issue-actions.js";
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

/** A dormant three-value system the edit tests mutate, then delete. */
async function threeValueSystem(client: ApiClient, projectId: string, prefix: string) {
  return createEstimate(client, CONTRACT_WORKSPACE, projectId, {
    name: uniqueName(prefix),
    type: "points",
    last_used: false,
    points: [
      { key: 1, value: "1" },
      { key: 2, value: "2" },
      { key: 3, value: "3" },
    ],
  });
}

function pointIdOf(system: Awaited<ReturnType<typeof threeValueSystem>>, value: string): string {
  const found = system.points.find((entry) => entry.value === value);
  expect(found).toBeDefined();
  return found?.id ?? "";
}

/**
 * The clear path deletes the value in the request but moves the affected
 * items off it asynchronously: the soft-delete sweep nulls the reference
 * from a queued Celery task, so an immediate re-read still carries the old
 * id. Poll until the item settles. This case therefore needs a running
 * worker (`celery -A pi_dash worker`) alongside the contract server — the
 * reassignment case above does not, because its `update()` is inline.
 */
async function pollIssueEstimate(
  client: ApiClient,
  projectId: string,
  issueId: string,
  settled: (entry: Awaited<ReturnType<typeof getIssue>>) => boolean
) {
  for (let attempt = 0; attempt < 40; attempt += 1) {
    const read = await getIssue(client, CONTRACT_WORKSPACE, projectId, issueId);
    if (settled(read)) return read;
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  return getIssue(client, CONTRACT_WORKSPACE, projectId, issueId);
}

describe.skipIf(!contractEnabled)("estimate system update", () => {
  it("patches the name with the full value set and persists across re-read", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const created = await threeValueSystem(client, projectId, "Contract update");
    try {
      const renamed = `${created.name} v2`;
      const updated = await updateProjectEstimate(client, CONTRACT_WORKSPACE, projectId, created.id, {
        name: renamed,
        points: [
          { id: pointIdOf(created, "3"), key: 1, value: "3" },
          { id: pointIdOf(created, "1"), key: 2, value: "1" },
          { id: pointIdOf(created, "2"), key: 3, value: "two" },
        ],
      });
      expect(updated.name).toBe(renamed);
      expect(updated.points).toHaveLength(3);
      const read = await getProjectEstimate(client, CONTRACT_WORKSPACE, projectId, created.id);
      expect(read.name).toBe(renamed);
      expect(orderEstimatePoints(read.points).map((entry) => entry.key)).toEqual([1, 2, 3]);
      expect(orderEstimatePoints(read.points).map((entry) => entry.value)).toEqual(["3", "1", "two"]);
    } finally {
      await deleteEstimate(client, CONTRACT_WORKSPACE, projectId, created.id).catch(() => 0);
    }
  });

  it("updates the known ids while ignoring an unknown one", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const created = await threeValueSystem(client, projectId, "Contract update unknown");
    try {
      const updated = await updateProjectEstimate(client, CONTRACT_WORKSPACE, projectId, created.id, {
        points: [
          { id: pointIdOf(created, "1"), key: 1, value: "one" },
          { id: "00000000-0000-4000-8000-000000000000", key: 9, value: "ghost" },
        ],
      });
      // No row created, none deleted: the unknown id simply did nothing.
      expect(updated.points).toHaveLength(3);
      const read = await getProjectEstimate(client, CONTRACT_WORKSPACE, projectId, created.id);
      expect(read.points.find((entry) => entry.id === pointIdOf(created, "1"))?.value).toBe("one");
      expect(read.points.some((entry) => entry.value === "ghost")).toBe(false);
    } finally {
      await deleteEstimate(client, CONTRACT_WORKSPACE, projectId, created.id).catch(() => 0);
    }
  });

  it("400s an empty value set sent raw", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const created = await threeValueSystem(client, projectId, "Contract update empty");
    try {
      const path = `/api/workspaces/${CONTRACT_WORKSPACE}/projects/${projectId}/estimates/${created.id}/`;
      const failure = await client
        .sendJson("PATCH", path, { json: { estimate: { name: "Empty" }, estimate_points: [] } })
        .then(
          () => null,
          (caught: unknown) => caught
        );
      expect(failure).toBeInstanceOf(ApiError);
      expect((failure as ApiError).status).toBe(400);
      expect((failure as ApiError).body).toEqual({ error: "Estimate points are required" });
    } finally {
      await deleteEstimate(client, CONTRACT_WORKSPACE, projectId, created.id).catch(() => 0);
    }
  });

  it("400s a missing value set sent raw", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const created = await threeValueSystem(client, projectId, "Contract update missing");
    try {
      const path = `/api/workspaces/${CONTRACT_WORKSPACE}/projects/${projectId}/estimates/${created.id}/`;
      const failure = await client.sendJson("PATCH", path, { json: { estimate: { name: "Missing" } } }).then(
        () => null,
        (caught: unknown) => caught
      );
      expect(failure).toBeInstanceOf(ApiError);
      expect((failure as ApiError).status).toBe(400);
      expect((failure as ApiError).body).toEqual({ error: "Estimate points are required" });
    } finally {
      await deleteEstimate(client, CONTRACT_WORKSPACE, projectId, created.id).catch(() => 0);
    }
  });

  it("200-ignores id-less rows sent raw, leaving the system unchanged", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const created = await threeValueSystem(client, projectId, "Contract update idless");
    try {
      const path = `/api/workspaces/${CONTRACT_WORKSPACE}/projects/${projectId}/estimates/${created.id}/`;
      // A 400 here would throw and fail the test: the raw call must answer 200.
      await client.sendJson("PATCH", path, { json: { estimate_points: [{ key: 1, value: "ghost" }] } });
      const read = await getProjectEstimate(client, CONTRACT_WORKSPACE, projectId, created.id);
      expect(read.points).toHaveLength(3);
      expect(read.points.some((entry) => entry.value === "ghost")).toBe(false);
      // The silent no-op is why the contract requires an id per row.
    } finally {
      await deleteEstimate(client, CONTRACT_WORKSPACE, projectId, created.id).catch(() => 0);
    }
  });
});

describe.skipIf(!contractEnabled)("estimate point writes", () => {
  it("creates one value and reads it back on the system", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const created = await threeValueSystem(client, projectId, "Contract point create");
    try {
      const point = await createProjectEstimatePoint(client, CONTRACT_WORKSPACE, projectId, created.id, {
        key: 4,
        value: "8",
      });
      expect(point.key).toBe(4);
      expect(point.value).toBe("8");
      const read = await getProjectEstimate(client, CONTRACT_WORKSPACE, projectId, created.id);
      expect(read.points.some((entry) => entry.id === point.id && entry.value === "8")).toBe(true);
    } finally {
      await deleteEstimate(client, CONTRACT_WORKSPACE, projectId, created.id).catch(() => 0);
    }
  });

  it("400s a single-point create missing its position, sent raw", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const created = await threeValueSystem(client, projectId, "Contract point nokey");
    try {
      const path = `/api/workspaces/${CONTRACT_WORKSPACE}/projects/${projectId}/estimates/${created.id}/estimate-points/`;
      const failure = await client.sendJson("POST", path, { json: { value: "8" } }).then(
        () => null,
        (caught: unknown) => caught
      );
      expect(failure).toBeInstanceOf(ApiError);
      expect((failure as ApiError).status).toBe(400);
      expect((failure as ApiError).body).toEqual({ error: "Key and value are required" });
    } finally {
      await deleteEstimate(client, CONTRACT_WORKSPACE, projectId, created.id).catch(() => 0);
    }
  });

  it("400s a single-point create missing its text, sent raw", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const created = await threeValueSystem(client, projectId, "Contract point novalue");
    try {
      const path = `/api/workspaces/${CONTRACT_WORKSPACE}/projects/${projectId}/estimates/${created.id}/estimate-points/`;
      const failure = await client.sendJson("POST", path, { json: { key: 4 } }).then(
        () => null,
        (caught: unknown) => caught
      );
      expect(failure).toBeInstanceOf(ApiError);
      expect((failure as ApiError).status).toBe(400);
      expect((failure as ApiError).body).toEqual({ error: "Key and value are required" });
    } finally {
      await deleteEstimate(client, CONTRACT_WORKSPACE, projectId, created.id).catch(() => 0);
    }
  });

  it("stores an over-long value on single-point create (NEWFRONT-347, pinned gap)", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const created = await threeValueSystem(client, projectId, "Contract point long");
    try {
      // 200, never 400: this endpoint skips the 20-character cap the bulk
      // and update paths enforce. The oracle pins the gap; the contract
      // must not promise the cap here.
      const long = "y".repeat(21);
      const point = await createProjectEstimatePoint(client, CONTRACT_WORKSPACE, projectId, created.id, {
        key: 4,
        value: long,
      });
      expect(point.value).toBe(long);
    } finally {
      await deleteEstimate(client, CONTRACT_WORKSPACE, projectId, created.id).catch(() => 0);
    }
  });

  it("partially updates one value and leaves the other field alone", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const created = await threeValueSystem(client, projectId, "Contract point patch");
    try {
      const target = pointIdOf(created, "2");
      const updated = await updateProjectEstimatePoint(client, CONTRACT_WORKSPACE, projectId, created.id, target, {
        value: "XL",
      });
      expect(updated.value).toBe("XL");
      expect(updated.key).toBe(2);
      const read = await getProjectEstimate(client, CONTRACT_WORKSPACE, projectId, created.id);
      expect(read.points.find((entry) => entry.id === target)?.value).toBe("XL");
    } finally {
      await deleteEstimate(client, CONTRACT_WORKSPACE, projectId, created.id).catch(() => 0);
    }
  });

  it("400s a point update whose value exceeds the 20-character cap", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const created = await threeValueSystem(client, projectId, "Contract point cap");
    try {
      const failure = await updateProjectEstimatePoint(
        client,
        CONTRACT_WORKSPACE,
        projectId,
        created.id,
        pointIdOf(created, "1"),
        { value: "x".repeat(21) }
      ).then(
        () => null,
        (caught: unknown) => caught
      );
      expect(failure).toBeInstanceOf(ApiError);
      expect((failure as ApiError).status).toBe(400);
      expect((failure as ApiError).body).toEqual({ non_field_errors: ["Value can't be more than 20 characters"] });
    } finally {
      await deleteEstimate(client, CONTRACT_WORKSPACE, projectId, created.id).catch(() => 0);
    }
  });

  it("400s an empty point update", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const created = await threeValueSystem(client, projectId, "Contract point empty");
    try {
      const failure = await updateProjectEstimatePoint(
        client,
        CONTRACT_WORKSPACE,
        projectId,
        created.id,
        pointIdOf(created, "1"),
        {}
      ).then(
        () => null,
        (caught: unknown) => caught
      );
      expect(failure).toBeInstanceOf(ApiError);
      expect((failure as ApiError).status).toBe(400);
      expect((failure as ApiError).body).toEqual({ non_field_errors: ["Estimate points are required"] });
    } finally {
      await deleteEstimate(client, CONTRACT_WORKSPACE, projectId, created.id).catch(() => 0);
    }
  });

  it("deletes one value with a replacement, moving its items and renumbering", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const created = await threeValueSystem(client, projectId, "Contract point reassign");
    const first = pointIdOf(created, "1");
    const second = pointIdOf(created, "2");
    const issue = await createIssue(client, CONTRACT_WORKSPACE, projectId, {
      name: uniqueName("Contract estimate move"),
      estimate_point: first,
    });
    try {
      const answer = await deleteProjectEstimatePoint(client, CONTRACT_WORKSPACE, projectId, created.id, first, {
        newEstimateId: second,
      });
      // The answer is the renumbered subset only: values 2 and 3 moved to 1 and 2.
      expect(answer.map((entry) => entry.key).sort()).toEqual([1, 2]);
      expect(answer.map((entry) => entry.value).sort()).toEqual(["2", "3"]);
      const read = await getProjectEstimate(client, CONTRACT_WORKSPACE, projectId, created.id);
      expect(read.points).toHaveLength(2);
      expect(orderEstimatePoints(read.points).map((entry) => entry.key)).toEqual([1, 2]);
      const moved = await getIssue(client, CONTRACT_WORKSPACE, projectId, issue.id);
      expect(moved.estimate_point).toBe(second);
    } finally {
      await deleteIssue(client, CONTRACT_WORKSPACE, projectId, issue.id).catch(() => undefined);
      await deleteEstimate(client, CONTRACT_WORKSPACE, projectId, created.id).catch(() => 0);
    }
  });

  it("deletes one value without a replacement, clearing its items", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const created = await threeValueSystem(client, projectId, "Contract point clear");
    const first = pointIdOf(created, "1");
    const issue = await createIssue(client, CONTRACT_WORKSPACE, projectId, {
      name: uniqueName("Contract estimate clear"),
      estimate_point: first,
    });
    try {
      await deleteProjectEstimatePoint(client, CONTRACT_WORKSPACE, projectId, created.id, first);
      const read = await getProjectEstimate(client, CONTRACT_WORKSPACE, projectId, created.id);
      expect(read.points).toHaveLength(2);
      expect(orderEstimatePoints(read.points).map((entry) => entry.key)).toEqual([1, 2]);
      const cleared = await pollIssueEstimate(client, projectId, issue.id, (entry) => entry.estimate_point === null);
      expect(cleared.estimate_point).toBeNull();
    } finally {
      await deleteIssue(client, CONTRACT_WORKSPACE, projectId, issue.id).catch(() => undefined);
      await deleteEstimate(client, CONTRACT_WORKSPACE, projectId, created.id).catch(() => 0);
    }
  });
});

describe.skipIf(!contractEnabled)("estimate edit refusals", () => {
  it("403s the editing endpoints for a project the caller does not belong to", async () => {
    const projectId = await bareProjectId(await signedIn());
    const unknown = "00000000-0000-4000-8000-000000000000";
    const refused = async (call: Promise<unknown>): Promise<ApiError | null> =>
      call.then(
        () => null,
        (caught: unknown) => caught as ApiError
      );

    const update = await refused(
      updateProjectEstimate(probe, CONTRACT_WORKSPACE, projectId, unknown, {
        points: [{ id: unknown, key: 1, value: "1" }],
      })
    );
    const create = await refused(
      createProjectEstimatePoint(probe, CONTRACT_WORKSPACE, projectId, unknown, { key: 1, value: "1" })
    );
    const patch = await refused(
      updateProjectEstimatePoint(probe, CONTRACT_WORKSPACE, projectId, unknown, unknown, { value: "1" })
    );
    const remove = await refused(deleteProjectEstimatePoint(probe, CONTRACT_WORKSPACE, projectId, unknown, unknown));
    for (const failure of [update, create, patch, remove]) {
      expect(failure).toBeInstanceOf(ApiError);
      expect(failure?.status).toBe(403);
    }
  });

  it("401s estimate reads and writes without a session", async () => {
    const projectId = await seededProjectId(await signedIn());
    const seeded = await listProjectEstimates(await signedIn(), CONTRACT_WORKSPACE, projectId);
    const systemId = seeded[0]?.id ?? "00000000-0000-4000-8000-000000000000";
    const stranger = contractClient();
    const refused = async (call: Promise<unknown>): Promise<ApiError | null> =>
      call.then(
        () => null,
        (caught: unknown) => caught as ApiError
      );

    const list = await refused(listProjectEstimates(stranger, CONTRACT_WORKSPACE, projectId));
    const write = await refused(
      createProjectEstimatePoint(stranger, CONTRACT_WORKSPACE, projectId, systemId, { key: 1, value: "1" })
    );
    for (const failure of [list, write]) {
      expect(failure).toBeInstanceOf(ApiError);
      expect(failure?.status).toBe(401);
    }
  });
});
