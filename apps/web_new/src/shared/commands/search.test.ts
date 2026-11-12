// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { renderHook, waitFor } from "@testing-library/react";
import type { ApiClient, SearchResults } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  countSearchHits,
  groupSearchResults,
  paletteSearchKeys,
  paletteSearchQueryOptions,
  useDebouncedValue,
} from "./search.js";

describe("useDebouncedValue", () => {
  it("holds the value back until it stays still", async () => {
    const { result, rerender } = renderHook(({ value }: { value: string }) => useDebouncedValue(value, 30), {
      initialProps: { value: "a" },
    });
    expect(result.current).toBe("a");
    rerender({ value: "ab" });
    expect(result.current).toBe("a");
    await waitFor(() => expect(result.current).toBe("ab"));
  });

  it("restarts the pause on every change", async () => {
    const { result, rerender } = renderHook(({ value }: { value: string }) => useDebouncedValue(value, 50), {
      initialProps: { value: "a" },
    });
    rerender({ value: "ab" });
    rerender({ value: "abc" });
    await waitFor(() => expect(result.current).toBe("abc"));
    expect(result.current).toBe("abc");
  });
});

describe("paletteSearchQueryOptions", () => {
  function stubClient(calls: Array<{ path: string; query: unknown }>): ApiClient {
    return {
      getJson: async (path: string, options?: { query?: unknown }) => {
        calls.push({ path, query: options?.query });
        return { results: {} };
      },
      parse: (_schema: unknown, payload: unknown) => payload,
    } as unknown as ApiClient;
  }

  it("keys by workspace, trimmed query, project and scope", () => {
    const options = paletteSearchQueryOptions({} as ApiClient, "acme", {
      query: "  Parity ",
      projectId: "p1",
      workspaceSearch: false,
    });
    expect(options.queryKey).toEqual(paletteSearchKeys.search("acme", "Parity", "p1", false));
    expect(options.enabled).toBe(true);
    expect(options.retry).toBe(false);
  });

  it("disables on a blank query so zero network fires", () => {
    const options = paletteSearchQueryOptions({} as ApiClient, "acme", {
      query: "   ",
      projectId: "p1",
      workspaceSearch: false,
    });
    expect(options.enabled).toBe(false);
  });

  it("passes the trimmed text with the project scope", async () => {
    const calls: Array<{ path: string; query: unknown }> = [];
    const options = paletteSearchQueryOptions(stubClient(calls), "acme", {
      query: "  Parity ",
      projectId: "p1",
      workspaceSearch: false,
    });
    await (options.queryFn as () => Promise<unknown>)();
    expect(calls).toHaveLength(1);
    expect(calls[0]?.path).toBe("/api/workspaces/acme/search/");
    expect(calls[0]?.query).toEqual({ search: "Parity", project_id: "p1", workspace_search: false });
  });

  it("omits the project id outside a project and widens on request", async () => {
    const calls: Array<{ path: string; query: unknown }> = [];
    const options = paletteSearchQueryOptions(stubClient(calls), "acme", {
      query: "Parity",
      projectId: null,
      workspaceSearch: true,
    });
    await (options.queryFn as () => Promise<unknown>)();
    expect(calls).toHaveLength(1);
    const query = calls[0]?.query as Record<string, unknown>;
    expect(query).toEqual({ search: "Parity", workspace_search: true });
    // Present-but-undefined: the client drops it from the wire query.
    expect(query["project_id"]).toBeUndefined();
  });
});

describe("groupSearchResults", () => {
  const ID = "123e4567-e89b-12d3-a456-426614174000";

  function results(overrides: SearchResults["results"]): SearchResults {
    return { results: overrides };
  }

  it("groups rows in palette order with contextual paths", () => {
    const groups = groupSearchResults(
      results({
        workspace: [{ name: "Acme", id: ID, slug: "acme" }],
        issue: [
          {
            name: "Fix login",
            id: ID,
            sequence_id: 12,
            project__identifier: "ATL",
            project_id: ID,
            workspace__slug: "acme",
          },
        ],
        project: [{ name: "Atlas", id: ID, identifier: "ATL", workspace__slug: "acme" }],
        cycle: [{ name: "Sprint 1", id: ID, project_id: ID, project__identifier: "ATL", workspace__slug: "acme" }],
        module: [{ name: "Auth", id: ID, project_id: ID, project__identifier: "ATL", workspace__slug: "acme" }],
        issue_view: [{ name: "Triage", id: ID, project_id: ID, project__identifier: "ATL", workspace__slug: "acme" }],
        page: [
          {
            name: "Spec",
            id: ID,
            project_ids: [ID],
            project_identifiers: ["ATL", 42, "BET"] as unknown as string[],
            workspace__slug: "acme",
          },
        ],
        intake: [
          {
            name: "Inbox item",
            id: ID,
            sequence_id: 3,
            project__identifier: "ATL",
            project_id: ID,
            workspace__slug: "acme",
          },
        ],
      })
    );
    expect(groups.map((group) => group.heading)).toEqual([
      "Issues",
      "Projects",
      "Cycles",
      "Modules",
      "Views",
      "Pages",
      "Intake items",
      "Workspaces",
    ]);
    const byHeading = new Map(groups.map((group) => [group.heading, group.hits[0]]));
    expect(byHeading.get("Issues")).toMatchObject({ key: `issue:${ID}`, title: "Fix login", path: "ATL-12" });
    expect(byHeading.get("Projects")).toMatchObject({ title: "Atlas", path: "ATL" });
    expect(byHeading.get("Cycles")).toMatchObject({ title: "Sprint 1", path: "ATL" });
    expect(byHeading.get("Modules")).toMatchObject({ title: "Auth", path: "ATL" });
    expect(byHeading.get("Views")).toMatchObject({ title: "Triage", path: "ATL" });
    expect(byHeading.get("Pages")).toMatchObject({ title: "Spec", path: "ATL, BET" });
    expect(byHeading.get("Intake items")).toMatchObject({ title: "Inbox item", path: "ATL-3" });
    expect(byHeading.get("Workspaces")).toMatchObject({ title: "Acme", path: "acme" });
    expect(countSearchHits(groups)).toBe(8);
  });

  it("skips absent and empty entities", () => {
    const groups = groupSearchResults(results({ issue: [], project: undefined }));
    expect(groups).toEqual([]);
    expect(countSearchHits(groups)).toBe(0);
  });

  it("degrades unreadable rows to blank paths", () => {
    const groups = groupSearchResults(
      results({
        issue: [
          {
            name: "Odd",
            id: ID,
            sequence_id: "twelve",
            project__identifier: "ATL",
            project_id: ID,
            workspace__slug: "acme",
          },
        ],
        page: [
          {
            name: "Loose",
            id: ID,
            project_ids: [],
            project_identifiers: "ATL",
            workspace__slug: "acme",
          },
        ],
        project: [{ name: "Nameless", id: ID, workspace__slug: "acme" }],
      } as unknown as SearchResults["results"])
    );
    const paths = new Map(groups.map((group) => [group.heading, group.hits[0]?.path]));
    expect(paths.get("Issues")).toBe("");
    expect(paths.get("Pages")).toBe("");
    expect(paths.get("Projects")).toBe("");
  });
});
