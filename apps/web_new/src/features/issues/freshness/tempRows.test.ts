// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient } from "@tanstack/react-query";
import type { IssueListItem } from "@pidash/api-client";
import { describe, expect, it } from "vitest";
import { insertTempRow, removeTempRow, tempIssueRow } from "./tempRows.js";

const PREFIX = ["ws", "acme", "issues", "list", "pid"] as const;
const OTHER = ["ws", "acme", "issues", "list", "other"] as const;

function row(id: string, name: string): IssueListItem {
  return {
    ...tempIssueRow(id, name),
    id,
    sequence_id: 42,
    project_id: "pid",
  };
}

function seed(client: QueryClient, key: readonly unknown[], rows: IssueListItem[]): void {
  client.setQueryData([...key, { search: "all" }], { results: rows, total_count: rows.length });
}

function cached(client: QueryClient, key: readonly unknown[]): { results: IssueListItem[]; total_count: number } {
  return client.getQueryData([...key, { search: "all" }]) as {
    results: IssueListItem[];
    total_count: number;
  };
}

describe("temp rows", () => {
  it("builds a synthetic row keyed by the temp id", () => {
    const temp = tempIssueRow("temp-1", "Draft title");
    expect(temp.id).toBe("temp-1");
    expect(temp.name).toBe("Draft title");
    expect(temp.sequence_id).toBe(0);
    expect(Date.parse(temp.created_at)).not.toBeNaN();
  });

  it("prepends the temp row to every cached list under the prefix", () => {
    const client = new QueryClient();
    seed(client, PREFIX, [row("real-1", "Real")]);
    seed(client, OTHER, [row("real-2", "Other")]);
    insertTempRow(client, [...PREFIX], tempIssueRow("temp-1", "Draft title"));
    expect(cached(client, PREFIX).results.map((entry) => entry.id)).toEqual(["temp-1", "real-1"]);
    expect(cached(client, PREFIX).total_count).toBe(2);
    expect(cached(client, OTHER).results.map((entry) => entry.id)).toEqual(["real-2"]);
    expect(cached(client, OTHER).total_count).toBe(1);
  });

  it("ignores duplicate inserts of the same temp row", () => {
    const client = new QueryClient();
    seed(client, PREFIX, []);
    insertTempRow(client, [...PREFIX], tempIssueRow("temp-1", "Draft title"));
    insertTempRow(client, [...PREFIX], tempIssueRow("temp-1", "Draft title"));
    expect(cached(client, PREFIX).results).toHaveLength(1);
    expect(cached(client, PREFIX).total_count).toBe(1);
  });

  it("removes the temp row and restores the total", () => {
    const client = new QueryClient();
    seed(client, PREFIX, [row("real-1", "Real")]);
    insertTempRow(client, [...PREFIX], tempIssueRow("temp-1", "Draft title"));
    removeTempRow(client, [...PREFIX], "temp-1");
    expect(cached(client, PREFIX).results.map((entry) => entry.id)).toEqual(["real-1"]);
    expect(cached(client, PREFIX).total_count).toBe(1);
  });

  it("leaves the cache alone when the temp id is absent", () => {
    const client = new QueryClient();
    seed(client, PREFIX, [row("real-1", "Real")]);
    removeTempRow(client, [...PREFIX], "temp-gone");
    expect(cached(client, PREFIX).results.map((entry) => entry.id)).toEqual(["real-1"]);
    expect(cached(client, PREFIX).total_count).toBe(1);
  });

  it("skips entries that are missing or not list shaped", () => {
    const client = new QueryClient();
    insertTempRow(client, [...PREFIX], tempIssueRow("temp-1", "Draft title"));
    removeTempRow(client, [...PREFIX], "temp-1");
    client.setQueryData([...PREFIX, "junk"], "not-a-record");
    client.setQueryData([...PREFIX, "norows"], { total_count: 1 });
    client.setQueryData([...PREFIX, "nototal"], { results: [] });
    insertTempRow(client, [...PREFIX], tempIssueRow("temp-1", "Draft title"));
    removeTempRow(client, [...PREFIX], "temp-1");
    expect(client.getQueryData([...PREFIX, "junk"])).toBe("not-a-record");
    expect(client.getQueryData([...PREFIX, "norows"])).toEqual({ total_count: 1 });
    expect(client.getQueryData([...PREFIX, "nototal"])).toEqual({ results: [] });
  });
});
