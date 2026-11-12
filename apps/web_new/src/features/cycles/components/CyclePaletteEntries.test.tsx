// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle palette entries mount (parity/specs/cycles.md §11, CYC-030):
// the live detail registers the favorite toggle plus copy-URL, the
// toggle writes through user-favorites and re-reads the cycle, and
// archived cycles register nothing while guests keep copy-URL only.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, waitFor } from "@testing-library/react";
import type { CycleRow as CycleRowData } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { ROLE_GUEST, ROLE_MEMBER } from "../../../core/session/permissions.js";
import { clearCommands, findCommand, listCommands } from "../../../shared/commands/registry.js";
import { makeCycleRow, CYCLES_CYCLE_ID, CYCLES_PROJECT_ID } from "../testing.js";
import { CyclePaletteEntries } from "./CyclePaletteEntries.js";

const WS = "acme";

let queryClient: QueryClient;
let posts: number;

function transport(url: string, init: { method?: string }): Promise<unknown> {
  const method = init.method ?? "GET";
  const respond = (status: number, body: unknown): unknown => ({
    status,
    url,
    headers: { get: () => null },
    text: async () => JSON.stringify(body),
  });
  if (url.includes("/user-favorites/") && method === "GET") return Promise.resolve(respond(200, []));
  if (url.includes("/user-favorites/") && method === "POST") {
    posts += 1;
    return Promise.resolve(
      respond(200, {
        id: "423e4567-e89b-12d3-a456-426614174003",
        entity_type: "cycle",
        entity_identifier: CYCLES_CYCLE_ID,
        entity_data: { name: "Cycle 7" },
        name: null,
        is_folder: false,
        sequence: 1,
        parent: null,
        workspace_id: "123e4567-e89b-12d3-a456-426614174000",
        project_id: CYCLES_PROJECT_ID,
      })
    );
  }
  return Promise.resolve(respond(200, []));
}

function renderEntries(cycle: CycleRowData, memberRole: number | null = ROLE_MEMBER): void {
  render(
    <QueryClientProvider client={queryClient}>
      <CyclePaletteEntries
        workspaceSlug={WS}
        projectId={CYCLES_PROJECT_ID}
        cycle={cycle}
        memberRole={memberRole}
        projectArchived={false}
      />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  createAppClient("https://api.test", transport as never);
  posts = 0;
});

afterEach(() => {
  resetAppClient();
  clearCommands();
  vi.unstubAllGlobals();
});

describe("CyclePaletteEntries", () => {
  it("registers the favorite toggle plus copy-URL on a live cycle", async () => {
    renderEntries(makeCycleRow({ is_favorite: false }));
    await waitFor(() => expect(findCommand(`context.cycle.${CYCLES_CYCLE_ID}.favorite`)).toBeDefined());
    expect(findCommand(`context.cycle.${CYCLES_CYCLE_ID}.favorite`)?.title).toBe("Add to favorites");
    await waitFor(() => expect(findCommand(`context.cycle.${CYCLES_CYCLE_ID}.copy-url`)).toBeDefined());
    expect(findCommand(`context.cycle.${CYCLES_CYCLE_ID}.copy-url`)?.title).toBe("Copy URL");
  });

  it("titles the toggle for removal on a starred cycle", async () => {
    renderEntries(makeCycleRow({ is_favorite: true }));
    await waitFor(() => expect(findCommand(`context.cycle.${CYCLES_CYCLE_ID}.favorite`)).toBeDefined());
    expect(findCommand(`context.cycle.${CYCLES_CYCLE_ID}.favorite`)?.title).toBe("Remove from favorites");
  });

  it("writes the toggle and re-reads the cycle after it settles", async () => {
    renderEntries(makeCycleRow({ is_favorite: false }));
    await waitFor(() => expect(findCommand(`context.cycle.${CYCLES_CYCLE_ID}.favorite`)).toBeDefined());
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    await act(async () => {
      await findCommand(`context.cycle.${CYCLES_CYCLE_ID}.favorite`)!.run();
    });
    await waitFor(() => expect(posts).toBe(1));
    await waitFor(() => expect(invalidate).toHaveBeenCalled());
    const keys = invalidate.mock.calls.map((call) => JSON.stringify(call[0]?.queryKey ?? null));
    expect(keys.some((key) => key.includes('"cycles"') && key.includes('"list"'))).toBe(true);
    expect(keys.some((key) => key.includes('"favorites"'))).toBe(true);
    invalidate.mockRestore();
  });

  it("registers nothing on archived cycles", async () => {
    renderEntries(makeCycleRow({ archived_at: "2026-03-01T00:00:00Z" }));
    await waitFor(() =>
      expect(queryClient.getQueryState(["ws", WS, "palette", "favorites"])?.dataUpdatedAt).toBeGreaterThan(0)
    );
    expect(listCommands()).toHaveLength(0);
  });

  it("keeps copy-URL only for guests", async () => {
    renderEntries(makeCycleRow({ is_favorite: false }), ROLE_GUEST);
    await waitFor(() => expect(findCommand(`context.cycle.${CYCLES_CYCLE_ID}.copy-url`)).toBeDefined());
    expect(findCommand(`context.cycle.${CYCLES_CYCLE_ID}.favorite`)).toBeUndefined();
  });

  it("reads a null role as guest level", async () => {
    renderEntries(makeCycleRow({ is_favorite: false }), null);
    await waitFor(() => expect(findCommand(`context.cycle.${CYCLES_CYCLE_ID}.copy-url`)).toBeDefined());
    expect(findCommand(`context.cycle.${CYCLES_CYCLE_ID}.favorite`)).toBeUndefined();
  });
});
