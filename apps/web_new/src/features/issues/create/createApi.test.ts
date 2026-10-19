// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Create-dialog network helpers (parity/specs/issues.md §11): the title
// generation timeout, the parent search query and key, and the parent
// candidate shaping. The dialog tests cover the hooks end to end.

import { describe, expect, it, vi } from "vitest";

import {
  PARENT_SEARCH_DEBOUNCE_MS,
  TitleTimeoutError,
  hitIdentifier,
  parentSearchKey,
  parentSearchQueryOptions,
  requestGeneratedTitle,
  selectParentCandidates,
  withTimeout,
} from "./createApi.js";

function hit(id: string, identifier = "ATL", sequence = 1) {
  return {
    id,
    name: `Row ${id}`,
    start_date: null,
    sequence_id: sequence,
    project__name: "Atlas",
    project__identifier: identifier,
    project_id: "11111111-1111-4111-8111-111111111111",
    workspace__slug: "acme",
    state__name: null,
    state__group: null,
    state__color: null,
  };
}

describe("withTimeout", () => {
  it("resolves with a timely answer", async () => {
    await expect(withTimeout(Promise.resolve("ok"), 1000)).resolves.toBe("ok");
  });

  it("rejects a late answer with the timeout error", async () => {
    await expect(withTimeout(new Promise<string>(() => undefined), 5)).rejects.toBeInstanceOf(TitleTimeoutError);
  });

  it("passes a rejection through unchanged", async () => {
    const failure = new Error("offline");
    await expect(withTimeout(Promise.reject(failure), 1000)).rejects.toBe(failure);
  });
});

describe("requestGeneratedTitle", () => {
  it("trims the assistant's answer", async () => {
    const client = {
      sendJson: vi.fn().mockResolvedValue({ data: { title: "  Named  " } }),
      parse: (_schema: unknown, payload: unknown) => payload,
    };
    await expect(requestGeneratedTitle(client as never, "acme", "<p>Body</p>")).resolves.toBe("Named");
    expect(client.sendJson).toHaveBeenCalledWith("POST", "/api/workspaces/acme/ai-assistant/generate-title/", {
      json: { description: "<p>Body</p>" },
    });
  });
});

describe("parent search", () => {
  it("accepts the half-second debounce from the row", () => {
    expect(PARENT_SEARCH_DEBOUNCE_MS).toBe(500);
  });

  it("keys each project and term separately", () => {
    expect(parentSearchKey("acme", "p", "par")).not.toEqual(parentSearchKey("acme", "p", ""));
    expect(parentSearchKey("acme", "p", "par")).toEqual(parentSearchKey("acme", "p", "par"));
  });

  it("asks the project search with the parent flag and no issue exclusion", async () => {
    const getJson = vi.fn().mockResolvedValue([]);
    const client = { getJson, parse: (_schema: unknown, payload: unknown) => payload };
    const options = parentSearchQueryOptions(client as never, "acme", "p", "par");
    await options.queryFn();
    expect(getJson).toHaveBeenCalledWith("/api/workspaces/acme/projects/p/search-issues/", {
      query: {
        search: "par",
        workspace_search: undefined,
        parent: "true",
        issue_relation: undefined,
        sub_issue: undefined,
        issue_id: undefined,
      },
    });
    expect(options.enabled).toBe(true);
  });

  it("stays disabled without a project", () => {
    const client = { getJson: vi.fn(), parse: (_schema: unknown, payload: unknown) => payload };
    expect(parentSearchQueryOptions(client as never, "acme", "", "").enabled).toBe(false);
  });

  it("labels a hit with its short code and sequence", () => {
    expect(hitIdentifier(hit("a", "NF", 42))).toBe("NF-42");
  });

  it("never offers the current parent again", () => {
    const rows = [hit("a"), hit("b")];
    expect(selectParentCandidates(rows, "a").map((row) => row.id)).toEqual(["b"]);
    expect(selectParentCandidates(rows, null)).toHaveLength(2);
  });
});
