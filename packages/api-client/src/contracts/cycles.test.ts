// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle detail read (ISS-224): the transfer prompt needs the progress
// snapshot plus the backlog/unstarted/started counts the detail
// serializer sends next to it.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import { getCycle, transferCycleIssues } from "./cycles.js";

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

const CYCLE_ROW = {
  id: UUID,
  project_id: UUID2,
  name: "Sprint 4",
  start_date: "2026-01-01T00:00:00Z",
  end_date: "2026-02-01T00:00:00Z",
  status: "COMPLETED",
  total_issues: 3,
  backlog_issues: 2,
  unstarted_issues: 1,
  started_issues: 0,
};

describe("getCycle", () => {
  it("reads the detail route and parses the counts", async () => {
    const { client, urls } = stubClient(CYCLE_ROW);
    const cycle = await getCycle(client, "acme", UUID2, UUID);
    expect(urls).toEqual([`/api/workspaces/acme/projects/${UUID2}/cycles/${UUID}/`]);
    expect(cycle.status).toBe("COMPLETED");
    expect(cycle.backlog_issues).toBe(2);
    expect(cycle.unstarted_issues).toBe(1);
    expect(cycle.progress_snapshot).toBeUndefined();
  });

  it("carries a frozen progress snapshot after a transfer", async () => {
    const { client } = stubClient({ ...CYCLE_ROW, progress_snapshot: { total_issues: 3, backlog_issues: 2 } });
    const cycle = await getCycle(client, "acme", UUID2, UUID);
    expect(cycle.progress_snapshot).toEqual({ total_issues: 3, backlog_issues: 2 });
  });

  it("accepts a null snapshot and an absent status", async () => {
    const { client } = stubClient({ ...CYCLE_ROW, status: undefined, progress_snapshot: null });
    const cycle = await getCycle(client, "acme", UUID2, UUID);
    expect(cycle.progress_snapshot).toBeNull();
    expect(cycle.status).toBeUndefined();
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
