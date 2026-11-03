// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the workspace export contracts: request URLs and
// bodies, the provider/status unions, the 400 paths, record and
// envelope parses, and unknown provider/status tolerance — all
// through a stub transport. Live-server proof lives in
// contract/exports.contract.test.ts.
import { describe, expect, it } from "vitest";
import { createClient } from "../client.js";
import { fakeResponse, recordingTransport } from "../client.test.js";
import type { RecordedCall } from "../client.test.js";
import { ApiError } from "../errors.js";
import {
  EXPORT_PROVIDERS,
  EXPORT_STATUSES,
  ExportProvider,
  ExportStatus,
  WorkspaceExport,
  WorkspaceExportReceipt,
  WorkspaceExportsPage,
  createWorkspaceExport,
  isKnownExportProvider,
  isKnownExportStatus,
  listWorkspaceExports,
} from "./exports.js";
import type { CreateWorkspaceExportInput } from "./exports.js";

const WS = "acme";
const ID = "123e4567-e89b-12d3-a456-426614174000";
const PROJECT_A = "223e4567-e89b-12d3-a456-426614174001";
const PROJECT_B = "323e4567-e89b-12d3-a456-426614174002";
const USER = "423e4567-e89b-12d3-a456-426614174003";
const DT = "2026-10-22T12:00:00.000Z";

function detail(overrides: Record<string, unknown> = {}) {
  return {
    id: USER,
    first_name: "Ada",
    last_name: "Lovelace",
    avatar: "AD",
    avatar_url: null,
    display_name: "Ada Lovelace",
    is_bot: false,
    ...overrides,
  };
}

function record(overrides: Record<string, unknown> = {}) {
  return {
    id: ID,
    created_at: DT,
    updated_at: DT,
    project: [PROJECT_A],
    provider: "csv",
    status: "completed",
    url: "https://files.test/exports/acme.csv",
    initiated_by: USER,
    initiated_by_detail: detail(),
    token: "export-token-1",
    created_by: USER,
    updated_by: null,
    ...overrides,
  };
}

function page(results: unknown[], overrides: Record<string, unknown> = {}) {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: results.length,
    next_cursor: "10:1:0",
    prev_cursor: "10:0:1",
    next_page_results: false,
    prev_page_results: false,
    count: results.length,
    total_pages: 1,
    total_results: results.length,
    extra_stats: null,
    results,
    ...overrides,
  };
}

function clientFor(handler: (call: RecordedCall) => { status: number; body: string }) {
  const transport = recordingTransport((call) => {
    const { status, body } = handler(call);
    return fakeResponse({ status, body });
  });
  const client = createClient({
    baseUrl: "https://api.test",
    transport,
    csrfToken: async () => "csrf",
  });
  return { client, transport };
}

function ok(value: unknown) {
  return clientFor(() => ({ status: 200, body: JSON.stringify(value) }));
}

describe("workspace export contracts", () => {
  it("posts the provider and projects with separate files derived from the count", async () => {
    const { client, transport } = ok({ message: "queued" });
    const input: CreateWorkspaceExportInput = { provider: "xlsx", project: [PROJECT_A, PROJECT_B] };
    const receipt = await createWorkspaceExport(client, WS, input);
    expect(receipt).toEqual({ message: "queued" });
    expect(transport.calls).toHaveLength(1);
    expect(transport.calls[0]?.url).toBe(`https://api.test/api/workspaces/${WS}/export-issues/`);
    expect(transport.calls[0]?.init.method).toBe("POST");
    expect(JSON.parse(transport.calls[0]?.init.body ?? "{}")).toEqual({
      provider: "xlsx",
      project: [PROJECT_A, PROJECT_B],
      multiple: true,
    });
  });

  it("sends multiple false for zero or one project", async () => {
    for (const project of [[], [PROJECT_A]]) {
      const { client, transport } = ok({ message: "queued" });
      await createWorkspaceExport(client, WS, { provider: "csv", project });
      expect(JSON.parse(transport.calls[0]?.init.body ?? "{}")).toMatchObject({ multiple: false });
    }
  });

  it("accepts each of the three providers on create", async () => {
    expect(EXPORT_PROVIDERS).toEqual(["csv", "xlsx", "json"]);
    for (const provider of EXPORT_PROVIDERS) {
      const { client, transport } = ok({ message: "queued" });
      await createWorkspaceExport(client, WS, { provider, project: [] });
      expect(JSON.parse(transport.calls[0]?.init.body ?? "{}")).toMatchObject({ provider });
      expect(ExportProvider.parse(provider)).toBe(provider);
    }
  });

  it("rejects an unknown provider id at the schema", () => {
    expect(() => ExportProvider.parse("pdf")).toThrow();
    expect(isKnownExportProvider("pdf")).toBe(false);
  });

  it("surfaces the server's provider refusal as the error message", async () => {
    const { client } = clientFor(() => ({
      status: 400,
      body: JSON.stringify({ error: "Provider 'pdf' not found." }),
    }));
    const failure = await createWorkspaceExport(client, WS, { provider: "csv", project: [] }).then(
      () => null,
      (error: unknown) => error
    );
    expect(failure).toBeInstanceOf(ApiError);
    expect((failure as ApiError).status).toBe(400);
    expect((failure as ApiError).message).toBe("Provider 'pdf' not found.");
  });

  it("rejects a receipt without the message", async () => {
    const { client } = ok({ ok: true });
    await expect(createWorkspaceExport(client, WS, { provider: "json", project: [] })).rejects.toThrow();
    expect(WorkspaceExportReceipt.parse({ message: "done" })).toEqual({ message: "done" });
  });

  it("reads the history with per_page, cursor and order_by", async () => {
    const { client, transport } = ok(page([record()]));
    const parsed = await listWorkspaceExports(client, WS, {
      perPage: 10,
      cursor: "10:0:0",
      orderBy: "-created_at",
    });
    expect(parsed.results).toHaveLength(1);
    const url = new URL(transport.calls[0]?.url ?? "");
    expect(`${url.origin}${url.pathname}`).toBe(`https://api.test/api/workspaces/${WS}/export-issues`);
    expect(url.searchParams.get("per_page")).toBe("10");
    expect(url.searchParams.get("cursor")).toBe("10:0:0");
    expect(url.searchParams.get("order_by")).toBe("-created_at");
  });

  it("omits query params the caller did not pass", async () => {
    const { client, transport } = ok(page([]));
    await listWorkspaceExports(client, WS, { perPage: 10, cursor: "10:0:0" });
    const url = new URL(transport.calls[0]?.url ?? "");
    expect(url.searchParams.has("order_by")).toBe(false);
    expect(url.searchParams.get("per_page")).toBe("10");
    expect(url.searchParams.get("cursor")).toBe("10:0:0");
  });

  it("surfaces the missing-params refusal as the error message", async () => {
    const { client } = clientFor(() => ({
      status: 400,
      body: JSON.stringify({ error: "per_page and cursor are required" }),
    }));
    const failure = await listWorkspaceExports(client, WS).then(
      () => null,
      (error: unknown) => error
    );
    expect(failure).toBeInstanceOf(ApiError);
    expect((failure as ApiError).status).toBe(400);
    expect((failure as ApiError).message).toBe("per_page and cursor are required");
  });

  it("parses a record with a null project list and a null file URL", async () => {
    const { client } = ok(page([record({ project: null, url: null, status: "queued" })]));
    const parsed = await listWorkspaceExports(client, WS, { perPage: 10, cursor: "10:0:0" });
    const row = parsed.results[0];
    expect(row?.project).toBeNull();
    expect(row?.url).toBeNull();
    expect(row?.status).toBe("queued");
    expect(row?.provider).toBe("csv");
  });

  it("parses a record whose initiator has no avatar URL and no email", async () => {
    const { client } = ok(page([record()]));
    const parsed = await listWorkspaceExports(client, WS, { perPage: 10, cursor: "10:0:0" });
    const initiator = parsed.results[0]?.initiated_by_detail;
    expect(initiator?.avatar_url).toBeNull();
    expect(initiator?.display_name).toBe("Ada Lovelace");
    expect(initiator).not.toHaveProperty("email");
  });

  it("keeps the whole paginator envelope including the end-of-list flags", async () => {
    const { client } = ok(
      page([record(), record({ id: PROJECT_A })], {
        total_count: 12,
        next_cursor: "10:1:0",
        prev_cursor: "10:0:1",
        next_page_results: true,
        prev_page_results: true,
        count: 2,
        total_pages: 2,
        total_results: 12,
      })
    );
    const parsed = await listWorkspaceExports(client, WS, { perPage: 10, cursor: "10:1:0" });
    expect(parsed.next_page_results).toBe(true);
    expect(parsed.prev_page_results).toBe(true);
    expect(parsed.next_cursor).toBe("10:1:0");
    expect(parsed.prev_cursor).toBe("10:0:1");
    expect(parsed.count).toBe(2);
    expect(parsed.total_count).toBe(12);
    expect(parsed.total_pages).toBe(2);
    expect(parsed.total_results).toBe(12);
    expect(parsed.grouped_by).toBeNull();
    expect(parsed.sub_grouped_by).toBeNull();
    expect(parsed.extra_stats).toBeNull();
  });

  it("requires the end-of-list flags on the envelope", () => {
    const envelope = page([record()]);
    expect(() => WorkspaceExportsPage.parse({ ...envelope, next_page_results: undefined })).toThrow();
    expect(() => WorkspaceExportsPage.parse({ ...envelope, prev_page_results: undefined })).toThrow();
  });

  it("accepts each of the four lifecycle states", async () => {
    expect(EXPORT_STATUSES).toEqual(["queued", "processing", "completed", "failed"]);
    for (const status of EXPORT_STATUSES) {
      expect(ExportStatus.parse(status)).toBe(status);
      expect(isKnownExportStatus(status)).toBe(true);
      const { client } = ok(page([record({ status })]));
      const parsed = await listWorkspaceExports(client, WS, { perPage: 10, cursor: "10:0:0" });
      expect(parsed.results[0]?.status).toBe(status);
    }
    for (const provider of EXPORT_PROVIDERS) {
      expect(isKnownExportProvider(provider)).toBe(true);
    }
  });

  it("tolerates unknown provider and status strings without crashing the page", async () => {
    const { client } = ok(page([record({ provider: "parquet", status: "archived" })]));
    const parsed = await listWorkspaceExports(client, WS, { perPage: 10, cursor: "10:0:0" });
    expect(parsed.results[0]?.provider).toBe("parquet");
    expect(parsed.results[0]?.status).toBe("archived");
    expect(isKnownExportProvider("parquet")).toBe(false);
    expect(isKnownExportStatus("archived")).toBe(false);
  });

  it("rejects a record missing its identity fields", () => {
    const row = record();
    expect(() => WorkspaceExport.parse({ ...row, provider: undefined })).toThrow();
    expect(() => WorkspaceExport.parse({ ...row, status: undefined })).toThrow();
    expect(() => WorkspaceExport.parse({ ...row, initiated_by_detail: undefined })).toThrow();
  });
});
