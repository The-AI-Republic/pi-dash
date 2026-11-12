// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page detail palette entries mount (shell SHELL-091): the owner
// registers lock, visibility, archive and favorite toggles plus
// copy-URL, the toggle writes through the page routes and re-reads
// the page, members keep the favorite toggle, and viewers without
// standing keep copy-URL only.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, waitFor } from "@testing-library/react";
import type { PageRow as PageRowData } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER } from "../../../core/session/permissions.js";
import { clearCommands, findCommand } from "../../../shared/commands/registry.js";
import { makePageRow, PAGES_PAGE_ID, PAGES_PROJECT_ID } from "../testing.js";
import { PageDetailPaletteEntries } from "./PageDetailPaletteEntries.js";

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
  if (url.includes("/favorite-pages/") && method === "POST") {
    posts += 1;
    return Promise.resolve(respond(204, ""));
  }
  return Promise.resolve(respond(200, []));
}

function renderEntries(page: PageRowData, isOwner: boolean, highestProjectRole: number | null): void {
  render(
    <QueryClientProvider client={queryClient}>
      <PageDetailPaletteEntries
        workspaceSlug={WS}
        projectId={PAGES_PROJECT_ID}
        page={page}
        isOwner={isOwner}
        highestProjectRole={highestProjectRole}
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

describe("PageDetailPaletteEntries", () => {
  it("registers lock, visibility, archive and favorite toggles plus copy-URL for the owner", async () => {
    renderEntries(makePageRow({ is_favorite: false }), true, ROLE_MEMBER);
    await waitFor(() => expect(findCommand(`context.page.${PAGES_PAGE_ID}.lock`)).toBeDefined());
    expect(findCommand(`context.page.${PAGES_PAGE_ID}.lock`)?.title).toBe("Lock");
    await waitFor(() => expect(findCommand(`context.page.${PAGES_PAGE_ID}.access`)).toBeDefined());
    expect(findCommand(`context.page.${PAGES_PAGE_ID}.access`)?.title).toBe("Make private");
    await waitFor(() => expect(findCommand(`context.page.${PAGES_PAGE_ID}.archive`)).toBeDefined());
    expect(findCommand(`context.page.${PAGES_PAGE_ID}.archive`)?.title).toBe("Archive");
    await waitFor(() => expect(findCommand(`context.page.${PAGES_PAGE_ID}.favorite`)).toBeDefined());
    expect(findCommand(`context.page.${PAGES_PAGE_ID}.favorite`)?.title).toBe("Add to favorites");
    await waitFor(() => expect(findCommand(`context.page.${PAGES_PAGE_ID}.copy-url`)).toBeDefined());
    expect(findCommand(`context.page.${PAGES_PAGE_ID}.copy-url`)?.title).toBe("Copy URL");
  });

  it("titles each toggle for the opposite state", async () => {
    renderEntries(
      makePageRow({ is_locked: true, access: 1, archived_at: "2026-03-01T00:00:00Z", is_favorite: true }),
      true,
      ROLE_ADMIN
    );
    await waitFor(() => expect(findCommand(`context.page.${PAGES_PAGE_ID}.lock`)).toBeDefined());
    expect(findCommand(`context.page.${PAGES_PAGE_ID}.lock`)?.title).toBe("Unlock");
    expect(findCommand(`context.page.${PAGES_PAGE_ID}.access`)?.title).toBe("Make public");
    expect(findCommand(`context.page.${PAGES_PAGE_ID}.archive`)?.title).toBe("Restore");
    expect(findCommand(`context.page.${PAGES_PAGE_ID}.favorite`)?.title).toBe("Remove from favorites");
  });

  it("writes the toggle and re-reads the page after it settles", async () => {
    renderEntries(makePageRow({ is_favorite: false }), true, ROLE_MEMBER);
    await waitFor(() => expect(findCommand(`context.page.${PAGES_PAGE_ID}.favorite`)).toBeDefined());
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    await act(async () => {
      await findCommand(`context.page.${PAGES_PAGE_ID}.favorite`)!.run();
    });
    await waitFor(() => expect(posts).toBe(1));
    await waitFor(() => {
      const keys = invalidate.mock.calls.map((call) => JSON.stringify(call[0]?.queryKey ?? null));
      expect(keys.some((key) => key.includes('"pages"') && key.includes('"list"'))).toBe(true);
      expect(keys.some((key) => key.includes('"pages"') && key.includes('"detail"'))).toBe(true);
      expect(keys.some((key) => key.includes('"favorites"'))).toBe(true);
    });
    invalidate.mockRestore();
  });

  it("keeps the favorite toggle plus copy-URL for a member without ownership", async () => {
    renderEntries(makePageRow({ is_favorite: false }), false, ROLE_MEMBER);
    await waitFor(() => expect(findCommand(`context.page.${PAGES_PAGE_ID}.favorite`)).toBeDefined());
    expect(findCommand(`context.page.${PAGES_PAGE_ID}.lock`)).toBeUndefined();
    expect(findCommand(`context.page.${PAGES_PAGE_ID}.access`)).toBeUndefined();
    expect(findCommand(`context.page.${PAGES_PAGE_ID}.archive`)).toBeUndefined();
    expect(findCommand(`context.page.${PAGES_PAGE_ID}.copy-url`)?.title).toBe("Copy URL");
  });

  it("keeps copy-URL only without standing", async () => {
    renderEntries(makePageRow({ is_favorite: false }), false, ROLE_GUEST);
    await waitFor(() => expect(findCommand(`context.page.${PAGES_PAGE_ID}.copy-url`)).toBeDefined());
    expect(findCommand(`context.page.${PAGES_PAGE_ID}.lock`)).toBeUndefined();
    expect(findCommand(`context.page.${PAGES_PAGE_ID}.favorite`)).toBeUndefined();
  });

  it("keeps copy-URL only for an outside viewer", async () => {
    renderEntries(makePageRow({ is_favorite: false }), false, null);
    await waitFor(() => expect(findCommand(`context.page.${PAGES_PAGE_ID}.copy-url`)).toBeDefined());
    expect(findCommand(`context.page.${PAGES_PAGE_ID}.favorite`)).toBeUndefined();
  });
});
