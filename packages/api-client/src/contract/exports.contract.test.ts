// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: the workspace issue-export endpoints against live Django
// (NEWFRONT-765). Covers the create call, the cursor-paged history
// read and both 400 paths. Created rows have no delete endpoint, so
// probes assert on presence and tolerate rows from earlier runs.
import { describe, expect, it } from "vitest";
import { ApiError } from "../errors.js";
import {
  createWorkspaceExport,
  isKnownExportProvider,
  isKnownExportStatus,
  listWorkspaceExports,
} from "../contracts/exports.js";
import type { ExportProvider } from "../contracts/exports.js";
import { getMe } from "../contracts/users.js";
import { CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";

describe.skipIf(!contractEnabled)("workspace exports contract", () => {
  it("creates an export and answers the acknowledgement", async () => {
    const client = await sharedSignedIn();
    const receipt = await createWorkspaceExport(client, CONTRACT_WORKSPACE, { provider: "csv", project: [] });
    expect(receipt.message.length).toBeGreaterThan(0);
  });

  it("refuses an unknown provider with the server message", async () => {
    const client = await sharedSignedIn();
    const failure = await createWorkspaceExport(client, CONTRACT_WORKSPACE, {
      provider: "pdf" as ExportProvider,
      project: [],
    }).then(
      () => null,
      (error: unknown) => error
    );
    expect(failure).toBeInstanceOf(ApiError);
    expect((failure as ApiError).status).toBe(400);
    expect((failure as ApiError).message).toContain("Provider");
  });

  it("reads the history page the UI's first read asks for", async () => {
    const client = await sharedSignedIn();
    const me = await getMe(client);
    await createWorkspaceExport(client, CONTRACT_WORKSPACE, { provider: "json", project: [] });
    const first = await listWorkspaceExports(client, CONTRACT_WORKSPACE, { perPage: 10, cursor: "10:0:0" });
    expect(first.total_results).toBeGreaterThanOrEqual(first.results.length);
    expect(first.count).toBe(first.results.length);
    expect(typeof first.next_page_results).toBe("boolean");
    expect(typeof first.prev_page_results).toBe("boolean");
    expect(first.next_cursor).toMatch(/^\d+:\d+:[01]$/);
    expect(first.prev_cursor).toMatch(/^\d+:-?\d+:[01]$/);
    expect(first.grouped_by).toBeNull();
    expect(first.sub_grouped_by).toBeNull();
    expect(first.extra_stats).toBeNull();
    expect(first.results.some((row) => row.initiated_by === me.id && row.provider === "json")).toBe(true);
    for (const row of first.results) {
      expect(row.initiated_by_detail).not.toHaveProperty("email");
      expect(isKnownExportProvider(row.provider)).toBe(true);
      expect(isKnownExportStatus(row.status)).toBe(true);
    }
  });

  it("requires both per_page and cursor on the history read", async () => {
    const client = await sharedSignedIn();
    const queries = [{}, { perPage: 10 }, { cursor: "10:0:0" }];
    for (const query of queries) {
      const failure = await listWorkspaceExports(client, CONTRACT_WORKSPACE, query).then(
        () => null,
        (error: unknown) => error
      );
      expect(failure).toBeInstanceOf(ApiError);
      expect((failure as ApiError).status).toBe(400);
      expect((failure as ApiError).message).toBe("per_page and cursor are required");
    }
  });
});
