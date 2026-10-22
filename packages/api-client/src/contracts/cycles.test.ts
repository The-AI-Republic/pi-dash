// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle progress read (ISS-224): the transfer prompt needs the
// backlog/unstarted/started counts the progress endpoint reports, the
// frozen ones included once a transfer has run.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import {
  archiveCycle,
  getArchivedCycle,
  getCycleProgress,
  listArchivedCycles,
  restoreCycle,
  transferCycleIssues,
} from "./cycles.js";

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

const PROGRESS = {
  total_issues: 3,
  backlog_issues: 2,
  unstarted_issues: 1,
  started_issues: 0,
};

describe("getCycleProgress", () => {
  it("reads the progress route and parses the counts", async () => {
    const { client, urls } = stubClient(PROGRESS);
    const progress = await getCycleProgress(client, "acme", UUID2, UUID);
    expect(urls).toEqual([`/api/workspaces/acme/projects/${UUID2}/cycles/${UUID}/progress/`]);
    expect(progress).toEqual(PROGRESS);
  });

  it("rejects a payload without the transferable counts", async () => {
    const { client } = stubClient({ total_issues: 3 });
    await expect(getCycleProgress(client, "acme", UUID2, UUID)).rejects.toThrow();
  });
});

describe("transferCycleIssues", () => {
  it("posts the target cycle to the transfer route and parses the message", async () => {
    const calls: { method?: string; url?: string; json?: unknown }[] = [];
    const client = {
      parse: ((schema: { parse: (value: unknown) => unknown }, value: unknown) =>
        schema.parse(value)) as ApiClient["parse"],
      sendJson: (async (method: string, url: string, options: { json?: unknown }) => {
        calls.push({ method, url, json: options.json });
        return { data: { message: "Work items have been transferred successfully" }, status: 200, url };
      }) as ApiClient["sendJson"],
    } as ApiClient;
    const result = await transferCycleIssues(client, "acme", UUID2, UUID, "323e4567-e89b-12d3-a456-426614174002");
    expect(calls).toEqual([
      {
        method: "POST",
        url: `/api/workspaces/acme/projects/${UUID2}/cycles/${UUID}/transfer-issues/`,
        json: { new_cycle_id: "323e4567-e89b-12d3-a456-426614174002" },
      },
    ]);
    expect(result.message).toBe("Work items have been transferred successfully");
  });
});

const ARCHIVED_ROW = {
  id: UUID,
  workspace_id: UUID2,
  project_id: UUID2,
  name: "Archived cycle",
  description: "",
  start_date: "2026-09-01T00:00:00Z",
  end_date: "2026-09-30T00:00:00Z",
  owned_by_id: UUID2,
  status: "COMPLETED",
  archived_at: "2026-10-01T00:00:00Z",
  is_favorite: false,
  total_issues: 4,
  completed_issues: 3,
  started_issues: 1,
  unstarted_issues: 0,
  cancelled_issues: 0,
  backlog_issues: 0,
  assignee_ids: [UUID2],
};

describe("listArchivedCycles", () => {
  it("reads the flat archived list and parses its rows", async () => {
    const { client, urls } = stubClient([ARCHIVED_ROW]);
    const rows = await listArchivedCycles(client, "acme", UUID2);
    expect(urls).toEqual([`/api/workspaces/acme/projects/${UUID2}/archived-cycles/`]);
    expect(rows).toHaveLength(1);
    expect(rows[0]?.status).toBe("COMPLETED");
    expect(rows[0]?.assignee_ids).toEqual([UUID2]);
  });

  it("rejects a row without the progress counts", async () => {
    const { total_issues: _dropped, ...rest } = ARCHIVED_ROW;
    void _dropped;
    const { client } = stubClient([rest]);
    await expect(listArchivedCycles(client, "acme", UUID2)).rejects.toThrow();
  });
});

const ARCHIVED_DETAIL = {
  ...ARCHIVED_ROW,
  sub_issues: 1,
  completed_estimate_points: 5,
  total_estimate_points: 8,
  estimate_distribution: {},
  distribution: { assignees: [], labels: [], completion_chart: {} },
};

describe("getArchivedCycle", () => {
  it("reads one archived cycle with its estimate summaries", async () => {
    const { client, urls } = stubClient(ARCHIVED_DETAIL);
    const detail = await getArchivedCycle(client, "acme", UUID2, UUID);
    expect(urls).toEqual([`/api/workspaces/acme/projects/${UUID2}/archived-cycles/${UUID}/`]);
    expect(detail.name).toBe("Archived cycle");
    expect(detail.total_estimate_points).toBe(8);
  });

  it("rejects a detail without the distributions", async () => {
    const { distribution: _dropped, ...rest } = ARCHIVED_DETAIL;
    void _dropped;
    const { client } = stubClient(rest);
    await expect(getArchivedCycle(client, "acme", UUID2, UUID)).rejects.toThrow();
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

describe("archiveCycle", () => {
  it("posts to the cycle archive route and parses the stamp", async () => {
    const { client, calls } = writeClient({ archived_at: "2026-10-21 12:00:00+00:00" }, 200);
    const stamp = await archiveCycle(client, "acme", UUID2, UUID);
    expect(calls).toEqual([{ method: "POST", url: `/api/workspaces/acme/projects/${UUID2}/cycles/${UUID}/archive/` }]);
    expect(stamp.archived_at).toBe("2026-10-21 12:00:00+00:00");
  });
});

describe("restoreCycle", () => {
  it("deletes the cycle archive route and resolves the status", async () => {
    const { client, calls } = writeClient(null, 204);
    const status = await restoreCycle(client, "acme", UUID2, UUID);
    expect(calls).toEqual([
      { method: "DELETE", url: `/api/workspaces/acme/projects/${UUID2}/cycles/${UUID}/archive/` },
    ]);
    expect(status).toBe(204);
  });
});
