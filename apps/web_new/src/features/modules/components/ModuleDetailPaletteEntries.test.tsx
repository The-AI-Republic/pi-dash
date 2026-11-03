// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module detail palette entries mount (shell SHELL-091): the live
// detail registers the member and status pickers, the favorite toggle
// plus copy-URL, the toggle writes through user-favorites and re-reads
// the module, and archived modules and guests keep copy-URL only.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, waitFor } from "@testing-library/react";
import type { ModuleRow as ModuleRowData } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { ROLE_GUEST, ROLE_MEMBER } from "../../../core/session/permissions.js";
import { clearCommands, findCommand } from "../../../shared/commands/registry.js";
import { makeModuleRow, MODULES_MODULE_ID, MODULES_PROJECT_ID } from "../testing.js";
import { ModuleDetailPaletteEntries } from "./ModuleDetailPaletteEntries.js";

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
        entity_type: "module",
        entity_identifier: MODULES_MODULE_ID,
        entity_data: { name: "Delivery" },
        name: null,
        is_folder: false,
        sequence: 1,
        parent: null,
        workspace_id: "123e4567-e89b-12d3-a456-426614174000",
        project_id: MODULES_PROJECT_ID,
      })
    );
  }
  return Promise.resolve(respond(200, []));
}

function renderEntries(module: ModuleRowData, memberRole: number | null = ROLE_MEMBER): void {
  render(
    <QueryClientProvider client={queryClient}>
      <ModuleDetailPaletteEntries
        workspaceSlug={WS}
        projectId={MODULES_PROJECT_ID}
        module={module}
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

describe("ModuleDetailPaletteEntries", () => {
  it("registers the member and status pickers, the favorite toggle plus copy-URL on a live module", async () => {
    renderEntries(makeModuleRow({ is_favorite: false }));
    await waitFor(() => expect(findCommand(`context.module.${MODULES_MODULE_ID}.members`)).toBeDefined());
    expect(findCommand(`context.module.${MODULES_MODULE_ID}.members`)?.title).toBe("Add/remove members");
    await waitFor(() => expect(findCommand(`context.module.${MODULES_MODULE_ID}.status`)).toBeDefined());
    expect(findCommand(`context.module.${MODULES_MODULE_ID}.status`)?.title).toBe("Change status");
    await waitFor(() => expect(findCommand(`context.module.${MODULES_MODULE_ID}.favorite`)).toBeDefined());
    expect(findCommand(`context.module.${MODULES_MODULE_ID}.favorite`)?.title).toBe("Add to favorites");
    await waitFor(() => expect(findCommand(`context.module.${MODULES_MODULE_ID}.copy-url`)).toBeDefined());
    expect(findCommand(`context.module.${MODULES_MODULE_ID}.copy-url`)?.title).toBe("Copy URL");
  });

  it("titles the toggle for removal on a starred module", async () => {
    renderEntries(makeModuleRow({ is_favorite: true }));
    await waitFor(() => expect(findCommand(`context.module.${MODULES_MODULE_ID}.favorite`)).toBeDefined());
    expect(findCommand(`context.module.${MODULES_MODULE_ID}.favorite`)?.title).toBe("Remove from favorites");
  });

  it("writes the toggle and re-reads the module after it settles", async () => {
    renderEntries(makeModuleRow({ is_favorite: false }));
    await waitFor(() => expect(findCommand(`context.module.${MODULES_MODULE_ID}.favorite`)).toBeDefined());
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    await act(async () => {
      await findCommand(`context.module.${MODULES_MODULE_ID}.favorite`)!.run();
    });
    await waitFor(() => expect(posts).toBe(1));
    await waitFor(() => {
      const keys = invalidate.mock.calls.map((call) => JSON.stringify(call[0]?.queryKey ?? null));
      expect(keys.some((key) => key.includes('"modules"') && key.includes('"list"'))).toBe(true);
      expect(keys.some((key) => key.includes('"favorites"'))).toBe(true);
    });
    invalidate.mockRestore();
  });

  it("registers copy-URL only on archived modules", async () => {
    renderEntries(makeModuleRow({ archived_at: "2026-03-01T00:00:00Z" }));
    await waitFor(() => expect(findCommand(`context.module.${MODULES_MODULE_ID}.copy-url`)).toBeDefined());
    expect(findCommand(`context.module.${MODULES_MODULE_ID}.members`)).toBeUndefined();
    expect(findCommand(`context.module.${MODULES_MODULE_ID}.status`)).toBeUndefined();
    expect(findCommand(`context.module.${MODULES_MODULE_ID}.favorite`)).toBeUndefined();
  });

  it("keeps copy-URL only for guests", async () => {
    renderEntries(makeModuleRow({ is_favorite: false }), ROLE_GUEST);
    await waitFor(() => expect(findCommand(`context.module.${MODULES_MODULE_ID}.copy-url`)).toBeDefined());
    expect(findCommand(`context.module.${MODULES_MODULE_ID}.members`)).toBeUndefined();
    expect(findCommand(`context.module.${MODULES_MODULE_ID}.favorite`)).toBeUndefined();
  });

  it("reads a null role as guest level", async () => {
    renderEntries(makeModuleRow({ is_favorite: false }), null);
    await waitFor(() => expect(findCommand(`context.module.${MODULES_MODULE_ID}.copy-url`)).toBeDefined());
    expect(findCommand(`context.module.${MODULES_MODULE_ID}.favorite`)).toBeUndefined();
  });
});
