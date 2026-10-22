// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived-module contracts against a stub client (no backend).
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import {
  archiveModule,
  getArchivedModule,
  listArchivedModules,
  listWorkspaceModules,
  restoreModule,
} from "./modules.js";

const UUID = "123e4567-e89b-12d3-a456-426614174000";
const UUID2 = "223e4567-e89b-12d3-a456-426614174001";

function stubClient(body: unknown): { client: ApiClient; urls: string[] } {
  const urls: string[] = [];
  const client = {
    parse: ((schema: { parse: (value: unknown) => unknown }, value: unknown) =>
      schema.parse(value)) as ApiClient["parse"],
    getJson: (async (url: string) => {
      urls.push(url);
      return body;
    }) as ApiClient["getJson"],
  } as ApiClient;
  return { client, urls };
}

const ARCHIVED_ROW = {
  id: UUID,
  workspace_id: UUID2,
  project_id: UUID2,
  name: "Archived module",
  description: "",
  description_text: null,
  description_html: null,
  start_date: "2026-09-01",
  target_date: "2026-09-30",
  status: "completed",
  lead_id: UUID2,
  member_ids: [UUID2],
  total_issues: 2,
  is_favorite: false,
  cancelled_issues: 0,
  completed_issues: 2,
  started_issues: 0,
  unstarted_issues: 0,
  backlog_issues: 0,
  created_at: "2026-09-01T00:00:00Z",
  updated_at: "2026-10-01T00:00:00Z",
  archived_at: "2026-10-01T00:00:00Z",
};

describe("listArchivedModules", () => {
  it("reads the flat archived list and parses its rows", async () => {
    const { client, urls } = stubClient([ARCHIVED_ROW]);
    const rows = await listArchivedModules(client, "acme", UUID2);
    expect(urls).toEqual([`/api/workspaces/acme/projects/${UUID2}/archived-modules/`]);
    expect(rows).toHaveLength(1);
    expect(rows[0]?.status).toBe("completed");
    expect(rows[0]?.member_ids).toEqual([UUID2]);
  });

  it("rejects a row without the progress counts", async () => {
    const { total_issues: _dropped, ...rest } = ARCHIVED_ROW;
    void _dropped;
    const { client } = stubClient([rest]);
    await expect(listArchivedModules(client, "acme", UUID2)).rejects.toThrow();
  });
});

const ARCHIVED_DETAIL = {
  ...ARCHIVED_ROW,
  link_module: [{ id: UUID2, title: "Spec", url: "https://example.test/spec" }],
  sub_issues: 0,
  total_estimate_points: 3,
  completed_estimate_points: 3,
  backlog_estimate_points: 0,
  unstarted_estimate_points: 0,
  started_estimate_points: 0,
  cancelled_estimate_points: 0,
  estimate_distribution: {},
  distribution: { assignees: [], labels: [], completion_chart: {} },
};

describe("getArchivedModule", () => {
  it("reads one archived module with its links and estimate sums", async () => {
    const { client, urls } = stubClient(ARCHIVED_DETAIL);
    const detail = await getArchivedModule(client, "acme", UUID2, UUID);
    expect(urls).toEqual([`/api/workspaces/acme/projects/${UUID2}/archived-modules/${UUID}/`]);
    expect(detail.name).toBe("Archived module");
    expect(detail.link_module).toHaveLength(1);
    expect(detail.completed_estimate_points).toBe(3);
  });

  it("rejects a detail without the distributions", async () => {
    const { distribution: _dropped, ...rest } = ARCHIVED_DETAIL;
    void _dropped;
    const { client } = stubClient(rest);
    await expect(getArchivedModule(client, "acme", UUID2, UUID)).rejects.toThrow();
  });
});

function writeClient(data: unknown, status: number): { client: ApiClient; calls: { method: string; url: string }[] } {
  const calls: { method: string; url: string }[] = [];
  const client = {
    parse: ((schema: { parse: (value: unknown) => unknown }, value: unknown) =>
      schema.parse(value)) as ApiClient["parse"],
    sendJson: (async (method: string, url: string) => {
      calls.push({ method, url });
      return { data, status, url };
    }) as ApiClient["sendJson"],
    delete: (async (url: string) => {
      calls.push({ method: "DELETE", url });
      return { status, url };
    }) as ApiClient["delete"],
  } as ApiClient;
  return { client, calls };
}

describe("archiveModule", () => {
  it("posts to the module archive route and parses the stamp", async () => {
    const { client, calls } = writeClient({ archived_at: "2026-10-21 12:00:00+00:00" }, 200);
    const stamp = await archiveModule(client, "acme", UUID2, UUID);
    expect(calls).toEqual([{ method: "POST", url: `/api/workspaces/acme/projects/${UUID2}/modules/${UUID}/archive/` }]);
    expect(stamp.archived_at).toBe("2026-10-21 12:00:00+00:00");
  });
});

describe("restoreModule", () => {
  it("deletes the module archive route and resolves the status", async () => {
    const { client, calls } = writeClient(null, 204);
    const status = await restoreModule(client, "acme", UUID2, UUID);
    expect(calls).toEqual([
      { method: "DELETE", url: `/api/workspaces/acme/projects/${UUID2}/modules/${UUID}/archive/` },
    ]);
    expect(status).toBe(204);
  });
});

describe("listWorkspaceModules", () => {
  const WORKSPACE_ROW = {
    id: UUID,
    workspace_id: UUID2,
    project_id: UUID2,
    name: "Alpha",
    status: "planned",
    total_issues: 2,
    completed_issues: 1,
  };

  it("reads the workspace module list without a project segment", async () => {
    const { client, urls } = stubClient([WORKSPACE_ROW]);
    const rows = await listWorkspaceModules(client, "acme");
    expect(urls).toEqual(["/api/workspaces/acme/modules/"]);
    expect(rows.map((row) => row.name)).toEqual(["Alpha"]);
  });

  it("accepts the favorite flag when the projection carries it", async () => {
    const { client } = stubClient([{ ...WORKSPACE_ROW, is_favorite: true }]);
    const rows = await listWorkspaceModules(client, "acme");
    expect(rows[0]?.is_favorite).toBe(true);
  });

  it("rejects a row without its identity", async () => {
    const { name: _dropped, ...rest } = WORKSPACE_ROW;
    void _dropped;
    const { client } = stubClient([rest]);
    await expect(listWorkspaceModules(client, "acme")).rejects.toThrow();
  });
});
