// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@pidash/api-client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...actual, getIssueByIdentifier: vi.fn(), getIssueMeta: vi.fn() };
});

import { ApiError, getIssueByIdentifier, getIssueMeta, type IssueLite } from "@pidash/api-client";

import { liteIssueQueryOptions, loadLiteIssue, parseBrowseKey, resolveShortLink } from "./browse.js";
import { issueKeys } from "./keys.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE_ID = "123e4567-e89b-12d3-a456-426614174000";

function liteRow(overrides: Partial<IssueLite> = {}): IssueLite {
  return {
    id: ISSUE_ID,
    sequence_id: 42,
    name: "Fix login",
    description_html: "<p></p>",
    sort_order: 1,
    project_id: PROJECT_ID,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    is_epic: false,
    is_intake: false,
    is_synced: false,
    archived_at: null,
    ...overrides,
  };
}

describe("parseBrowseKey", () => {
  it("splits on the last dash", () => {
    expect(parseBrowseKey("ACME-42")).toEqual({ identifier: "ACME", sequenceId: 42 });
    expect(parseBrowseKey("AC-ME-42")).toEqual({ identifier: "AC-ME", sequenceId: 42 });
  });

  it("rejects keys without a numeric tail", () => {
    expect(parseBrowseKey("")).toBeNull();
    expect(parseBrowseKey("ACME")).toBeNull();
    expect(parseBrowseKey("ACME-")).toBeNull();
    expect(parseBrowseKey("-42")).toBeNull();
    expect(parseBrowseKey("ACME-4x")).toBeNull();
  });
});

describe("loadLiteIssue", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("resolves the key through the lite identifier fetch", async () => {
    vi.mocked(getIssueByIdentifier).mockResolvedValue(liteRow());

    const resolution = await loadLiteIssue("client" as never, "acme", "ACME-42");

    expect(getIssueByIdentifier).toHaveBeenCalledWith("client", "acme", "ACME", 42);
    expect(resolution.lite?.id).toBe(ISSUE_ID);
    expect(resolution.lite?.project_id).toBe(PROJECT_ID);
  });

  it("resolves archived rows without a fallback", async () => {
    vi.mocked(getIssueByIdentifier).mockResolvedValue(liteRow({ archived_at: "2024-03-04" }));

    const resolution = await loadLiteIssue("client" as never, "acme", "ACME-42");

    expect(resolution.lite?.archived_at).toBe("2024-03-04");
  });

  it("returns null for malformed keys without calling the API", async () => {
    expect(await loadLiteIssue("client" as never, "acme", "no-key-here")).toEqual({ lite: null });
    expect(getIssueByIdentifier).not.toHaveBeenCalled();
  });

  it.each([400, 403, 404])("maps status %s to an unknown key", async (status) => {
    vi.mocked(getIssueByIdentifier).mockRejectedValue(new ApiError({ status, code: "http", message: "no" }));

    expect(await loadLiteIssue("client" as never, "acme", "ACME-42")).toEqual({ lite: null });
  });

  it("rethrows non-API failures so the route can offer a retry", async () => {
    const failure = new Error("boom");
    vi.mocked(getIssueByIdentifier).mockRejectedValue(failure);

    await expect(loadLiteIssue("client" as never, "acme", "ACME-42")).rejects.toBe(failure);
  });

  it("rethrows server failures so the route can offer a retry", async () => {
    const failure = new ApiError({ status: 500, code: "http", message: "bad" });
    vi.mocked(getIssueByIdentifier).mockRejectedValue(failure);

    await expect(loadLiteIssue("client" as never, "acme", "ACME-42")).rejects.toBe(failure);
  });
});

describe("liteIssueQueryOptions", () => {
  it("keys the resolution by workspace and key", () => {
    const options = liteIssueQueryOptions("client" as never, "acme", "ACME-42");
    expect(options.queryKey).toEqual(issueKeys.browse("acme", "ACME-42"));
  });
});

describe("resolveShortLink", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("builds the browse key from the issue meta", async () => {
    vi.mocked(getIssueMeta).mockResolvedValue({ project_identifier: "WEB", sequence_id: 7 });

    const target = await resolveShortLink("client" as never, "acme", PROJECT_ID, ISSUE_ID);

    expect(getIssueMeta).toHaveBeenCalledWith("client", "acme", PROJECT_ID, ISSUE_ID);
    expect(target).toEqual({ issueKey: "WEB-7" });
  });

  it.each([400, 403, 404, 500])("maps status %s to an unresolvable link", async (status) => {
    vi.mocked(getIssueMeta).mockRejectedValue(new ApiError({ status, code: "http", message: "no" }));

    await expect(resolveShortLink("client" as never, "acme", PROJECT_ID, ISSUE_ID)).resolves.toBeNull();
  });

  it("maps transport failures to an unresolvable link", async () => {
    vi.mocked(getIssueMeta).mockRejectedValue(new Error("boom"));

    await expect(resolveShortLink("client" as never, "acme", PROJECT_ID, ISSUE_ID)).resolves.toBeNull();
  });
});
