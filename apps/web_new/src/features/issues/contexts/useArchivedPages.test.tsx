// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Paging for the archived list (parity/specs/archives.md §2.1–2.2,
// ARCH-005/006). A flat list appends cursor pages and offers more; a
// grouped list keeps one cursor chain per bucket so a small bucket does
// not wait for the global window, and either chain resets when the
// expression, grouping or order changes.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { GroupedIssueListResponse, IssueListResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { useArchivedPages } from "./archivedIssues.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const STATE_A = "323e4567-e89b-12d3-a456-426614174002";
const STATE_B = "423e4567-e89b-12d3-a456-426614174003";
const ID_A = "523e4567-e89b-12d3-a456-426614174004";
const ID_B = "623e4567-e89b-12d3-a456-426614174005";
const ID_C = "723e4567-e89b-12d3-a456-426614174006";

function row(id: string, name: string, sequence: number) {
  return {
    id,
    name,
    state_id: STATE_A,
    sort_order: sequence,
    completed_at: "2026-01-01T00:00:00Z",
    priority: "none" as const,
    sequence_id: sequence,
    project_id: PROJECT,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-02T00:00:00Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: "2026-01-03",
  };
}

function flat(rows: ReturnType<typeof row>[], next: string, nextPage: boolean, total = rows.length): IssueListResponse {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: total,
    count: rows.length,
    total_results: total,
    next_cursor: next,
    prev_cursor: "",
    next_page_results: nextPage,
    prev_page_results: false,
    results: rows,
  };
}

function grouped(
  buckets: Record<string, { results: ReturnType<typeof row>[]; total_results: number }>,
  total: number
): GroupedIssueListResponse {
  return {
    grouped_by: "state_id",
    sub_grouped_by: null,
    total_count: total,
    next_cursor: "",
    prev_cursor: "",
    next_page_results: false,
    prev_page_results: false,
    count: Object.values(buckets).reduce((sum, bucket) => sum + bucket.results.length, 0),
    total_results: total,
    results: buckets,
  };
}

function mount(respond: (url: URL) => unknown, status = 200) {
  createAppClient("https://api.test", (async (url: string) => {
    const body = respond(new URL(url));
    return {
      status,
      url,
      headers: { get: () => null },
      text: async () => (status === 200 ? JSON.stringify(body) : "{}"),
    };
  }) as never);
  return wrapperFor();
}

function usePages(expression: object, groupBy: string | null, orderBy: string) {
  return useArchivedPages(SLUG, PROJECT, expression as never, groupBy, orderBy, new Map([[STATE_A, "Todo"]]), true);
}

beforeEach(() => resetAppClient());
afterEach(() => resetAppClient());

describe("useArchivedPages flat list", () => {
  it("appends cursor pages and de-duplicates overlapping rows", async () => {
    const wrapper = mount((url) => {
      const cursor = url.searchParams.get("cursor");
      if (cursor === null) return flat([row(ID_A, "First", 1), row(ID_B, "Second", 2)], "100:1:0", true, 3);
      return flat([row(ID_B, "Second", 2), row(ID_C, "Third", 3)], "", false, 3);
    });
    const { result } = renderHook(() => usePages({}, null, "-created_at"), { wrapper });
    await waitFor(() => expect(result.current.sections[0]?.rows).toHaveLength(2));
    expect(result.current.sections[0]?.rows.map((entry) => entry.id)).toEqual([ID_A, ID_B]);
    expect(result.current.total).toBe(3);
    expect(result.current.hasMore).toBe(true);
    act(() => result.current.loadMore());
    await waitFor(() => expect(result.current.sections[0]?.rows).toHaveLength(3));
    expect(result.current.sections[0]?.rows.map((entry) => entry.id)).toEqual([ID_A, ID_B, ID_C]);
    expect(result.current.hasMore).toBe(false);
  });

  it("does not ask for another page when the envelope says there is none", async () => {
    const wrapper = mount(() => flat([row(ID_A, "Only", 1)], "", false, 1));
    const { result } = renderHook(() => usePages({}, null, "sort_order"), { wrapper });
    await waitFor(() => expect(result.current.sections[0]?.rows).toHaveLength(1));
    expect(result.current.hasMore).toBe(false);
    act(() => result.current.loadMore());
    expect(result.current.sections[0]?.rows).toHaveLength(1);
  });

  it("ignores a page whose envelope carries no cursor", async () => {
    const wrapper = mount(() => flat([row(ID_A, "Only", 1)], "", true, 2));
    const { result } = renderHook(() => usePages({}, null, "sort_order"), { wrapper });
    await waitFor(() => expect(result.current.sections[0]?.rows).toHaveLength(1));
    expect(result.current.hasMore).toBe(true);
    act(() => result.current.loadMore());
    expect(result.current.sections[0]?.rows).toHaveLength(1);
  });

  it("does not append the same cursor twice", async () => {
    const wrapper = mount(() => flat([row(ID_A, "First", 1)], "100:1:0", true, 5));
    const { result } = renderHook(() => usePages({}, null, "sort_order"), { wrapper });
    await waitFor(() => expect(result.current.sections[0]?.rows).toHaveLength(1));
    act(() => {
      result.current.loadMore();
      result.current.loadMore();
    });
    await waitFor(() => expect(result.current.sections[0]?.rows).toHaveLength(1));
  });

  it("reports the error state when the first read fails", async () => {
    const wrapper = mount(() => ({}), 500);
    const { result } = renderHook(() => usePages({}, null, "sort_order"), { wrapper });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(result.current.isPending).toBe(false);
  });
});

describe("useArchivedPages grouped list", () => {
  it("merges the buckets and pages a single section", async () => {
    let sectionReads = 0;
    const wrapper = mount((url) => {
      const cursor = url.searchParams.get("cursor");
      if (cursor === null) {
        return grouped(
          {
            [STATE_A]: { results: [row(ID_A, "First", 1)], total_results: 2 },
            [STATE_B]: { results: [row(ID_B, "Second", 2)], total_results: 1 },
          },
          3
        );
      }
      sectionReads += 1;
      return grouped({ [STATE_A]: { results: [row(ID_C, "Third", 3)], total_results: 2 } }, 2);
    });
    const { result } = renderHook(() => usePages({}, "state", "sort_order"), { wrapper });
    await waitFor(() => expect(result.current.sections).toHaveLength(2));
    expect(result.current.grouped).toBe(true);
    expect(result.current.sections.map((section) => section.key)).toEqual([STATE_A, STATE_B]);
    expect(result.current.total).toBe(3);
    const first = result.current.sections[0];
    expect(result.current.sectionMore(first!)).toBe(true);
    act(() => result.current.loadMoreSection(first!));
    await waitFor(() => expect(result.current.sections[0]?.rows).toHaveLength(2));
    expect(sectionReads).toBe(1);
    expect(result.current.sectionMore(result.current.sections[0]!)).toBe(false);
  });

  it("ignores a section paging request with nothing left", async () => {
    const wrapper = mount(() => grouped({ [STATE_A]: { results: [row(ID_A, "First", 1)], total_results: 1 } }, 1));
    const { result } = renderHook(() => usePages({}, "state", "sort_order"), { wrapper });
    await waitFor(() => expect(result.current.sections).toHaveLength(1));
    act(() => result.current.loadMoreSection(result.current.sections[0]!));
    expect(result.current.sections[0]?.rows).toHaveLength(1);
  });

  it("does not page the same section cursor twice", async () => {
    const wrapper = mount(() => grouped({ [STATE_A]: { results: [row(ID_A, "First", 1)], total_results: 5 } }, 5));
    const { result } = renderHook(() => usePages({}, "state", "sort_order"), { wrapper });
    await waitFor(() => expect(result.current.sections).toHaveLength(1));
    act(() => {
      result.current.loadMoreSection(result.current.sections[0]!);
      result.current.loadMoreSection(result.current.sections[0]!);
    });
    await waitFor(() => expect(result.current.sections[0]?.rows).toHaveLength(1));
  });

  it("skips empty buckets unless the switch asks for them", async () => {
    const wrapper = mount(() =>
      grouped(
        {
          [STATE_A]: { results: [], total_results: 0 },
          [STATE_B]: { results: [row(ID_B, "One", 1)], total_results: 1 },
        },
        1
      )
    );
    const hidden = renderHook(() => useArchivedPages(SLUG, PROJECT, {}, "state", "sort_order", new Map(), false), {
      wrapper,
    });
    await waitFor(() => expect(hidden.result.current.sections).toHaveLength(1));
    expect(hidden.result.current.sections.map((section) => section.key)).toEqual([STATE_B]);
    const shown = renderHook(() => useArchivedPages(SLUG, PROJECT, {}, "state", "sort_order", new Map(), true), {
      wrapper,
    });
    await waitFor(() => expect(shown.result.current.sections).toHaveLength(2));
    expect(shown.result.current.sections.map((section) => section.key)).toEqual([STATE_A, STATE_B]);
  });
});

describe("useArchivedPages reset", () => {
  it("drops the cursor chain when the grouping changes", async () => {
    const seen: Array<string | null> = [];
    const wrapper = mount((url) => {
      seen.push(url.searchParams.get("cursor"));
      if (url.searchParams.get("group_by") !== null) {
        return grouped({ [STATE_A]: { results: [row(ID_A, "First", 1)], total_results: 1 } }, 1);
      }
      return flat([row(ID_A, "First", 1)], "100:1:0", true, 2);
    });
    const { result, rerender } = renderHook(
      ({ group }: { group: string | null }) => usePages({}, group, "sort_order"),
      { wrapper, initialProps: { group: null as string | null } }
    );
    await waitFor(() => expect(result.current.sections[0]?.rows).toHaveLength(1));
    act(() => result.current.loadMore());
    await waitFor(() => expect(seen).toContain("100:1:0"));
    rerender({ group: "state" });
    await waitFor(() => expect(result.current.grouped).toBe(true));
    await waitFor(() => expect(result.current.sections[0]?.rows.map((entry) => entry.id)).toEqual([ID_A]));
    // The grouped read carries no cursor: the flat chain was dropped.
    expect(seen[seen.length - 1]).toBeNull();
  });
});

function wrapperFor() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return ({ children }: { children: React.ReactNode }): React.ReactElement => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
}
