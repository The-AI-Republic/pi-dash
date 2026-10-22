// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Estimates contract against a stub client (no backend): URLs, request
// bodies, response parses, the required create position, and the ordering
// selectors every estimates consumer reuses.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import { ApiError } from "../errors.js";
import {
  EstimateCreateBody,
  createEstimate,
  deleteEstimate,
  getProjectEstimate,
  listProjectEstimatePoints,
  listProjectEstimates,
  orderEstimatePoints,
  selectActiveEstimate,
  selectArchivedEstimates,
} from "./estimates.js";
import type { Estimate, EstimateCreateInput } from "./estimates.js";

const WS = "acme";
const PROJECT = "123e4567-e89b-12d3-a456-426614174000";
const OTHER_PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const WORKSPACE = "323e4567-e89b-12d3-a456-426614174002";
const SYSTEM = "423e4567-e89b-12d3-a456-426614174003";
const POINT_A = "523e4567-e89b-12d3-a456-426614174004";
const POINT_B = "623e4567-e89b-12d3-a456-426614174005";
const EARLY = "2026-10-01T10:00:00Z";
const LATE = "2026-10-16T15:32:05.479661+09:00";

function point(id: string, key: number, value: string, description = ""): Estimate["points"][number] {
  return { id, key, value, description };
}

function system(overrides: Partial<Estimate> = {}): Estimate {
  return {
    id: SYSTEM,
    workspace: WORKSPACE,
    project: PROJECT,
    name: "Points",
    description: "",
    type: "points",
    last_used: true,
    points: [point(POINT_A, 1, "2"), point(POINT_B, 2, "1")],
    created_at: EARLY,
    updated_at: EARLY,
    created_by: null,
    updated_by: null,
    ...overrides,
  };
}

interface Calls {
  get: string[];
  post: { url: string; body: unknown }[];
  delete: string[];
}

function stubClient(getBody: unknown, calls: Calls = { get: [], post: [], delete: [] }): ApiClient {
  return {
    getJson: (async (url: string) => {
      calls.get.push(url);
      return getBody;
    }) as ApiClient["getJson"],
    sendJson: (async (method: string, url: string, options: { json?: unknown } = {}) => {
      calls.post.push({ url: `${method} ${url}`, body: options.json });
      return { data: getBody, status: 200, url };
    }) as ApiClient["sendJson"],
    delete: (async (url: string) => {
      calls.delete.push(url);
      return { status: 204, url, headers: { get: () => null }, text: async () => "" };
    }) as ApiClient["delete"],
    parse: ((schema: { parse: (value: unknown) => unknown }, value: unknown, what: string) => {
      try {
        return schema.parse(value);
      } catch (error) {
        throw new ApiError({
          status: 0,
          code: "contract",
          message: `${what} did not match its contract`,
          cause: error,
        });
      }
    }) as ApiClient["parse"],
  } as ApiClient;
}

describe("listProjectEstimates", () => {
  it("reads every system of the project with its nested values", async () => {
    const calls: Calls = { get: [], post: [], delete: [] };
    const list = await listProjectEstimates(stubClient([system()], calls), WS, PROJECT);
    expect(calls.get).toEqual([`/api/workspaces/${WS}/projects/${PROJECT}/estimates/`]);
    expect(list).toHaveLength(1);
    expect(list[0]?.points.map((entry) => entry.key)).toEqual([1, 2]);
  });

  it("accepts an empty project list", async () => {
    await expect(listProjectEstimates(stubClient([]), WS, PROJECT)).resolves.toEqual([]);
  });

  it("wraps a shape violation as a contract error", async () => {
    const broken = [{ ...system(), last_used: "yes" }];
    const failure = await listProjectEstimates(stubClient(broken), WS, PROJECT).catch((error: unknown) => error);
    expect(failure).toBeInstanceOf(ApiError);
    expect((failure as ApiError).code).toBe("contract");
  });
});

describe("getProjectEstimate", () => {
  it("reads one system through its project-scoped address", async () => {
    const calls: Calls = { get: [], post: [], delete: [] };
    const read = await getProjectEstimate(stubClient(system(), calls), WS, PROJECT, SYSTEM);
    expect(calls.get).toEqual([`/api/workspaces/${WS}/projects/${PROJECT}/estimates/${SYSTEM}/`]);
    expect(read.id).toBe(SYSTEM);
  });
});

describe("deleteEstimate", () => {
  it("resolves the 204 status of the whole-system delete", async () => {
    const calls: Calls = { get: [], post: [], delete: [] };
    await expect(deleteEstimate(stubClient(null, calls), WS, PROJECT, SYSTEM)).resolves.toBe(204);
    expect(calls.delete).toEqual([`/api/workspaces/${WS}/projects/${PROJECT}/estimates/${SYSTEM}/`]);
  });
});

describe("listProjectEstimatePoints", () => {
  it("reads the linked system's values", async () => {
    const calls: Calls = { get: [], post: [], delete: [] };
    const points = await listProjectEstimatePoints(stubClient([point(POINT_A, 1, "2")], calls), WS, PROJECT);
    expect(calls.get).toEqual([`/api/workspaces/${WS}/projects/${PROJECT}/project-estimates/`]);
    expect(points.map((entry) => entry.value)).toEqual(["2"]);
  });

  it("accepts an unlinked project's empty answer", async () => {
    await expect(listProjectEstimatePoints(stubClient([]), WS, PROJECT)).resolves.toEqual([]);
  });
});

describe("createEstimate", () => {
  it("sends the estimate envelope with the caller's 1..n positions", async () => {
    const calls: Calls = { get: [], post: [], delete: [] };
    const created = await createEstimate(stubClient(system(), calls), WS, PROJECT, {
      name: "Points",
      type: "points",
      last_used: true,
      points: [
        { key: 1, value: "2" },
        { key: 2, value: "1", description: "one" },
        { key: 3, value: "3" },
      ],
    });
    expect(calls.post).toEqual([
      {
        url: `POST /api/workspaces/${WS}/projects/${PROJECT}/estimates/`,
        body: {
          estimate: { name: "Points", type: "points", last_used: true },
          estimate_points: [
            { key: 1, value: "2" },
            { key: 2, value: "1", description: "one" },
            { key: 3, value: "3" },
          ],
        },
      },
    ]);
    expect(created.id).toBe(SYSTEM);
  });

  it("leaves an omitted name to the server and defaults the rest", async () => {
    const calls: Calls = { get: [], post: [], delete: [] };
    await createEstimate(stubClient(system({ name: "qwertyuiop" }), calls), WS, PROJECT, {});
    expect(calls.post[0]?.body).toEqual({ estimate: {}, estimate_points: [] });
  });

  it("refuses a point row written without a position", async () => {
    const calls: Calls = { get: [], post: [], delete: [] };
    const withoutKey = [{ value: "2" }] as unknown as NonNullable<EstimateCreateInput["points"]>;
    await expect(createEstimate(stubClient(system(), calls), WS, PROJECT, { points: withoutKey })).rejects.toThrow();
    expect(calls.post).toEqual([]);
  });

  it.each([1.5, 0, -1])("refuses the position %s", async (key) => {
    const calls: Calls = { get: [], post: [], delete: [] };
    await expect(
      createEstimate(stubClient(system(), calls), WS, PROJECT, { points: [{ key, value: "2" }] })
    ).rejects.toThrow();
    expect(calls.post).toEqual([]);
  });

  it("cannot construct a body that omits key (type contract)", () => {
    // @ts-expect-error — a point body always carries its 1-based position.
    const body: EstimateCreateBody = { estimate: {}, estimate_points: [{ value: "2" }] };
    expect(body.estimate_points).toHaveLength(1);
  });
});

describe("selectActiveEstimate", () => {
  it("returns the in-use system and nothing when none is flagged", () => {
    const active = system();
    expect(selectActiveEstimate([system({ id: OTHER_PROJECT, last_used: false, created_at: LATE }), active])?.id).toBe(
      SYSTEM
    );
    expect(selectActiveEstimate([system({ last_used: false })])).toBeUndefined();
  });
});

describe("selectArchivedEstimates", () => {
  it("keeps only unused systems, newest created first", () => {
    const older = system({ id: SYSTEM, last_used: false, created_at: EARLY });
    const newer = system({ id: OTHER_PROJECT, last_used: false, created_at: LATE });
    const active = system({ id: WORKSPACE, last_used: true, created_at: "2026-10-20T00:00:00Z" });
    expect(selectArchivedEstimates([older, active, newer]).map((entry) => entry.id)).toEqual([OTHER_PROJECT, SYSTEM]);
  });

  it("breaks a created-at tie by id so the order stays stable", () => {
    const left = system({ id: SYSTEM, last_used: false, created_at: EARLY });
    const right = system({ id: OTHER_PROJECT, last_used: false, created_at: EARLY });
    expect(selectArchivedEstimates([right, left]).map((entry) => entry.id)).toEqual([SYSTEM, OTHER_PROJECT].sort());
  });

  it("returns an empty list when every system is in use", () => {
    expect(selectArchivedEstimates([system()])).toEqual([]);
  });
});

describe("orderEstimatePoints", () => {
  it("sorts by position without mutating the input and keeps ties stable", () => {
    const first = point(POINT_A, 2, "one");
    const second = point(POINT_B, 1, "two");
    const tie = point(SYSTEM, 2, "three");
    const input = [first, second, tie];
    expect(orderEstimatePoints(input).map((entry) => entry.value)).toEqual(["two", "one", "three"]);
    expect(input.map((entry) => entry.value)).toEqual(["one", "two", "three"]);
  });

  it("leaves an already ordered list alone", () => {
    const ordered = [point(POINT_A, 1, "1"), point(POINT_B, 2, "2")];
    expect(orderEstimatePoints(ordered).map((entry) => entry.key)).toEqual([1, 2]);
  });
});
