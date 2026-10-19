// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Duplicate seam (parity/specs/issues.md §11 ISS-121, §23): the OSS
// edition answers none, so no button or panel can mount, and the length
// guard never spends a search on a very short title. A flag-on build
// searches after the debounce and surfaces only exact title matches.

// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, renderHook, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import type { IssueSearchHit } from "@pidash/api-client";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import {
  DEDUPE_DEBOUNCE_MS,
  DuplicatePanel,
  ISSUE_DEDUPE_FLAG,
  duplicateSearchKey,
  isDebouncing,
  selectDuplicateMatches,
  shouldSearchDuplicates,
  useDuplicateMatches,
} from "./dedupe.js";

function hit(id: string, name: string): IssueSearchHit {
  return {
    id,
    name,
    start_date: null,
    sequence_id: 1,
    project__name: "Atlas",
    project__identifier: "ATL",
    project_id: "323e4567-e89b-12d3-a456-426614174002",
    workspace__slug: "acme",
    state__name: "Todo",
    state__group: "unstarted",
    state__color: "#5e6ad2",
  };
}

const PROJECT = "323e4567-e89b-12d3-a456-426614174002";
const HIT_A = "723e4567-e89b-12d3-a456-426614174006";
const HIT_B = "823e4567-e89b-12d3-a456-426614174007";
const HIT_C = "923e4567-e89b-12d3-a456-426614174008";

describe("useDuplicateMatches", () => {
  function wrapper({ children }: { children: React.ReactNode }) {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    return React.createElement(QueryClientProvider, { client }, children);
  }

  it("returns no matches and keeps the surface off in the community build", () => {
    const result = renderHook(
      () => useDuplicateMatches({ workspaceSlug: "acme", projectId: PROJECT, title: "Fix login", enabled: false }),
      { wrapper }
    ).result.current;
    expect(result).toEqual({ matches: [], isLoading: false, isEnabled: false });
  });

  it("stays off for a short title even when the flag is on", () => {
    const result = renderHook(
      () => useDuplicateMatches({ workspaceSlug: "acme", projectId: PROJECT, title: "ab", enabled: true }),
      { wrapper }
    ).result.current;
    expect(result.isEnabled).toBe(false);
  });

  it("exposes the stable flag the cloud build replaces behind", () => {
    expect(ISSUE_DEDUPE_FLAG).toBe("issue-dedupe");
    expect(DEDUPE_DEBOUNCE_MS).toBeGreaterThan(0);
  });
});

describe("useDuplicateMatches with the edition flag on", () => {
  afterEach(() => {
    resetAppClient();
  });

  function serve(hits: IssueSearchHit[]) {
    const calls: string[] = [];
    createAppClient("https://api.test", (async (url: string) => {
      calls.push(url);
      if (url.endsWith("/auth/get-csrf-token/")) {
        return {
          status: 200,
          url,
          headers: { get: () => null },
          text: async () => JSON.stringify({ csrf_token: "c" }),
        };
      }
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(hits) };
    }) as never);
    return calls;
  }

  function wrapper({ children }: { children: React.ReactNode }) {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    return React.createElement(QueryClientProvider, { client }, children);
  }

  it("debounces the title, searches the project and surfaces exact matches", async () => {
    const calls = serve([hit(HIT_A, "Fix login")]);
    const { result } = renderHook(
      () => useDuplicateMatches({ workspaceSlug: "acme", projectId: PROJECT, title: "Fix login", enabled: true }),
      { wrapper }
    );
    await waitFor(() => expect(result.current.matches).toHaveLength(1), { timeout: 3000, interval: 25 });
    expect(result.current.isEnabled).toBe(true);
    expect(calls.some((url) => url.includes(`/projects/${PROJECT}/search-issues/`))).toBe(true);
    expect(calls.some((url) => url.includes("search=Fix+login"))).toBe(true);
  });

  it("drops rows with a different title or from another project", async () => {
    serve([
      hit(HIT_B, "Fix logout"),
      { ...hit(HIT_C, "Fix login"), project_id: "423e4567-e89b-12d3-a456-426614174003" },
    ]);
    const { result } = renderHook(
      () => useDuplicateMatches({ workspaceSlug: "acme", projectId: PROJECT, title: "Fix login", enabled: true }),
      { wrapper }
    );
    await waitFor(() => expect(result.current.matches).toHaveLength(0));
    expect(result.current.isEnabled).toBe(false);
  });
});

describe("duplicate search shaping", () => {
  it("keys the cache by workspace, project and term", () => {
    expect(duplicateSearchKey("acme", "p", "Fix")).toEqual([...duplicateSearchKey("acme", "p", "Fix")]);
    expect(duplicateSearchKey("acme", "p", "Fix")).toContain("Fix");
  });

  it("treats a title as pending until the debounced term settles", () => {
    expect(isDebouncing("Fix l", "Fix")).toBe(true);
    expect(isDebouncing("Fix", "Fix")).toBe(false);
  });

  it("matches only an exact, case-insensitive title in the same project", () => {
    const rows = [
      hit(HIT_A, "Fix Login"),
      hit(HIT_B, "Fix login again"),
      { ...hit(HIT_C, "Fix login"), project_id: "423e4567-e89b-12d3-a456-426614174003" },
    ];
    expect(selectDuplicateMatches(rows, " fix LOGIN ", PROJECT).map((row) => row.id)).toEqual([HIT_A]);
    expect(selectDuplicateMatches(rows, "   ", PROJECT)).toEqual([]);
  });
});

describe("DuplicatePanel", () => {
  it("renders nothing without matches", () => {
    const { container } = render(React.createElement(DuplicatePanel, { matches: [] }));
    expect(container).toBeEmptyDOMElement();
  });

  it("counts one match in the singular and opens it on request", async () => {
    const user = userEvent.setup();
    render(React.createElement(DuplicatePanel, { matches: [hit("1", "Fix login")] }));
    const count = screen.getByRole("button", { name: "1 possible duplicate" });
    expect(count).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("complementary", { name: "Possible duplicates" })).not.toBeInTheDocument();
    await user.click(count);
    expect(count).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("Fix login")).toBeInTheDocument();
    expect(screen.getByRole("complementary", { name: "Possible duplicates" })).toBeInTheDocument();
  });

  it("pluralizes the count and links each match in a new tab", async () => {
    const user = userEvent.setup();
    render(React.createElement(DuplicatePanel, { matches: [hit("1", "Fix login"), hit("2", "Fix logout")] }));
    await user.click(screen.getByRole("button", { name: "2 possible duplicates" }));
    expect(screen.getByText("Fix logout")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Fix login" })).toHaveAttribute("target", "_blank");
    expect(screen.getByRole("link", { name: "Fix login" })).toHaveAttribute("href", "/acme/browse/ATL-1");
  });
});

describe("shouldSearchDuplicates", () => {
  it("needs the flag and a title of some substance", () => {
    expect(shouldSearchDuplicates("Fix login", true)).toBe(true);
    expect(shouldSearchDuplicates("Fix login", false)).toBe(false);
    expect(shouldSearchDuplicates("  ab  ", true)).toBe(false);
    expect(shouldSearchDuplicates("   ", true)).toBe(false);
  });
});
