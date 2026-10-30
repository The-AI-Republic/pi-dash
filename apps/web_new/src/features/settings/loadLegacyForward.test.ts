// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Legacy-forward loader (PROF-026; main#670): it runs the signed-in gate
// first, then resolves the tab's workspace from the settings and workspace
// reads. A failed read leaves the page usable: settings fall back to the
// remembered fallback slug, the workspace list to undefined.
import type { QueryClient } from "@tanstack/react-query";
import { isNotFound } from "@tanstack/react-router";
import { ApiError, createClient } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { CREATE_WORKSPACE_PATH } from "../../core/session/index.js";
import { loadLegacySettingsForward } from "./loadLegacyForward.js";

const ME = { is_password_autoset: false, id: "u1" };

function queryClient(fixtures: { me?: unknown; settings?: unknown; workspaces?: unknown }): QueryClient {
  return {
    ensureQueryData: async (options: { queryKey: readonly unknown[] }) => {
      const key = options.queryKey[1];
      const value = key === "profile" ? { is_onboarded: true } : fixtures[key as "me" | "settings" | "workspaces"];
      if (value === undefined) throw new Error(`no fixture for ${String(key)}`);
      if (value instanceof Error) throw value;
      return value;
    },
  } as unknown as QueryClient;
}

const apiClient = createClient({
  baseUrl: "https://api.test",
  transport: async () => {
    throw new Error("no fetch");
  },
});

const WORKSPACES = [
  { id: "w1", name: "Acme", slug: "acme", logo_url: null, total_members: 1, role: 20 },
  { id: "w2", name: "Globex", slug: "globex", logo_url: null, total_members: 1, role: 20 },
];

describe("loadLegacySettingsForward", () => {
  it("lands inside the last workspace", async () => {
    await expect(
      loadLegacySettingsForward({
        queryClient: queryClient({
          me: ME,
          settings: { workspace: { last_workspace_slug: "globex", fallback_workspace_slug: "acme" } },
          workspaces: WORKSPACES,
        }),
        apiClient,
        pathname: "/settings/profile/security/",
        search: "?tab=all",
      })
    ).resolves.toBe("/globex/settings/account/security/?tab=all");
  });

  it("keeps the page usable when the settings read fails", async () => {
    // No settings row means no remembered workspace, so the reader lands on
    // workspace creation rather than a blank settings page.
    await expect(
      loadLegacySettingsForward({
        queryClient: queryClient({
          me: ME,
          settings: new Error("settings down"),
          workspaces: WORKSPACES,
        }),
        apiClient,
        pathname: "/profile/preferences",
        search: "",
      })
    ).resolves.toBe(CREATE_WORKSPACE_PATH);
  });

  it("sends a reader whose workspaces cannot be read to workspace creation", async () => {
    await expect(
      loadLegacySettingsForward({
        queryClient: queryClient({
          me: ME,
          settings: { workspace: { last_workspace_slug: "acme", fallback_workspace_slug: null } },
          workspaces: new Error("list down"),
        }),
        apiClient,
        pathname: "/settings/",
        search: "",
      })
    ).resolves.toBe(CREATE_WORKSPACE_PATH);
  });

  it("answers the not-found page outside the forward families", async () => {
    const thrown = await loadLegacySettingsForward({
      queryClient: queryClient({
        me: ME,
        settings: { workspace: { last_workspace_slug: "acme", fallback_workspace_slug: null } },
        workspaces: WORKSPACES,
      }),
      apiClient,
      pathname: "/profile/a/b",
      search: "",
    }).catch((error: unknown) => error);
    expect(isNotFound(thrown)).toBe(true);
  });

  it("gates the forward for a visitor", async () => {
    const thrown = await loadLegacySettingsForward({
      queryClient: queryClient({ me: new ApiError({ status: 401, code: "http", message: "signed out" }) }),
      apiClient,
      pathname: "/settings/",
      search: "",
    }).catch((error: unknown) => error);
    expect((thrown as { options: { to?: string } }).options.to).toBe("/");
  });
});
