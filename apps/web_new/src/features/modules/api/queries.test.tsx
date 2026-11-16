// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The list read sits under the module list key, fetches through the
// api-client contract and never refetches on window focus (§8.5,
// MOD-039): a change another member makes appears on the next load.

import { QueryClient } from "@tanstack/react-query";
import type { ModuleRow } from "@pidash/api-client";
import { describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ listProjectModules: vi.fn() }));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, listProjectModules: mocks.listProjectModules };
});

import { LIST_STALE_TIME_MS } from "../../../core/query/index.js";
import { moduleKeys } from "./keys.js";
import { modulesListQueryOptions } from "./queries.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";

function rows(): ModuleRow[] {
  return [
    {
      id: "323e4567-e89b-12d3-a456-426614174002",
      workspace_id: "123e4567-e89b-12d3-a456-426614174000",
      project_id: PROJECT,
      name: "Delivery",
      description: "",
      start_date: null,
      target_date: null,
      status: "backlog",
      lead_id: null,
      member_ids: [],
      sort_order: 0,
      is_favorite: false,
      total_issues: 0,
      completed_issues: 0,
      cancelled_issues: 0,
      started_issues: 0,
      unstarted_issues: 0,
      backlog_issues: 0,
      created_at: "2026-01-02T03:04:05Z",
      updated_at: "2026-02-03T04:05:06Z",
    },
  ];
}

describe("modulesListQueryOptions", () => {
  it("fetches the project's rows under the list key and never on focus", async () => {
    mocks.listProjectModules.mockResolvedValue(rows());
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const options = modulesListQueryOptions({ stub: true } as never, "acme", PROJECT);
    expect(options.queryKey).toEqual(moduleKeys.list("acme", PROJECT));
    expect(options.staleTime).toBe(LIST_STALE_TIME_MS);
    expect(options.refetchOnWindowFocus).toBe(false);
    const fetched = await queryClient.fetchQuery(options);
    expect(mocks.listProjectModules).toHaveBeenCalledWith({ stub: true }, "acme", PROJECT);
    expect(fetched.map((row) => row.name)).toEqual(["Delivery"]);
  });
});
