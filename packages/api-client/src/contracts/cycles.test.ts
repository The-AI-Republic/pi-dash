// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle progress read (ISS-224): the transfer prompt needs the
// backlog/unstarted/started counts the progress endpoint reports, the
// frozen ones included once a transfer has run.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import { getCycleProgress, transferCycleIssues } from "./cycles.js";

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
