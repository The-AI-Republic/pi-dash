// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The cycle/module prefetch reads the stored sub-issue flag before the
// first page and starts the layout chunk alongside it (ISS-010/013).

import { describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  ensure: vi.fn(),
  load: vi.fn(async () => ({ default: () => null })),
  page: vi.fn((..._args: unknown[]) => ({ queryKey: ["page"] })),
  prefs: vi.fn((..._args: unknown[]) => ({ queryKey: ["prefs"] })),
}));

vi.mock("../prefs/prefsQuery.js", () => ({ prefsQueryOptions: mocks.prefs }));
vi.mock("./lazy.js", () => ({ loadListLayout: mocks.load }));
vi.mock("./pages.js", () => ({ listPageQueryOptions: mocks.page }));

import { prefetchEntityList } from "./prefetch.js";

const ENTITY = { scope: "cycle", projectId: "project-1", cycleId: "cycle-1" } as const;

function context(displayFilters: Record<string, unknown>) {
  const queryClient = { ensureQueryData: mocks.ensure };
  mocks.ensure.mockImplementation(async (options: { queryKey: string[] }) =>
    options.queryKey[0] === "prefs" ? { display_filters: displayFilters } : undefined
  );
  return { context: { queryClient, apiClient: { stub: true } }, queryClient };
}

describe("prefetchEntityList", () => {
  it("reads the stored sub-issue flag before starting the page and layout chunk", async () => {
    const { context: ctx, queryClient } = context({ sub_issue: false });
    await prefetchEntityList(ctx as never, "acme", ENTITY);
    expect(mocks.prefs).toHaveBeenCalledWith(expect.anything(), "acme", ENTITY);
    expect(mocks.page).toHaveBeenCalledWith(expect.anything(), "acme", {
      entity: ENTITY,
      groupBy: null,
      orderBy: "sort_order",
      showSubIssues: false,
    });
    expect(mocks.load).toHaveBeenCalledTimes(1);
    expect(queryClient.ensureQueryData).toHaveBeenCalledTimes(2);
  });

  it("includes child rows when the stored record predates the flag", async () => {
    const { context: ctx } = context({});
    await prefetchEntityList(ctx as never, "acme", ENTITY);
    expect(mocks.page).toHaveBeenCalledWith(
      expect.anything(),
      "acme",
      expect.objectContaining({ showSubIssues: true })
    );
  });
});
