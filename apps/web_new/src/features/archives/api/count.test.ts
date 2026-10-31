// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi } from "vitest";

vi.mock("@pidash/api-client", () => ({
  listArchivedIssues: vi.fn(),
}));

import { listArchivedIssues } from "@pidash/api-client";

import { archivedCountQueryOptions } from "./count.js";
import { archivesKeys } from "./keys.js";

const CLIENT = { name: "stub-client" } as never;

describe("archivesKeys", () => {
  it("scopes the archives reads under the workspace and project", () => {
    expect(archivesKeys.all("acme", "p1")).toEqual(["ws", "acme", "archives", "p1"]);
    expect(archivesKeys.count("acme", "p1")).toEqual(["ws", "acme", "archives", "p1", "count"]);
  });
});

describe("archivedCountQueryOptions", () => {
  it("reads the total off the first archived-issues page", async () => {
    vi.mocked(listArchivedIssues).mockResolvedValue({ total_count: 7, results: [] } as never);
    const options = archivedCountQueryOptions(CLIENT, "acme", "p1");

    expect(options.queryKey).toEqual(["ws", "acme", "archives", "p1", "count"]);
    if (!options.queryFn) throw new Error("missing queryFn");
    await expect(options.queryFn({} as never)).resolves.toBe(7);
    expect(listArchivedIssues).toHaveBeenCalledWith(CLIENT, "acme", "p1");
  });
});
