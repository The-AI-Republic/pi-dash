// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Settings gate (parity/specs/profile.md §5): visitors bounce to the entry
// with their return path, unfinished members continue onboarding, and a
// finished member passes through.
import type { QueryClient } from "@tanstack/react-query";
import { isRedirect } from "@tanstack/react-router";
import { createClient } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { enforceProfileSettingsGate } from "./guard.js";

const ME = {
  id: "123e4567-e89b-12d3-a456-426614174000",
  email: "ada@example.com",
  username: "ada",
  first_name: "Ada",
  last_name: "Lovelace",
  display_name: "ada",
  avatar: "",
  avatar_url: null,
  cover_image: null,
  cover_image_url: null,
  date_joined: "2024-01-01T00:00:00Z",
  user_timezone: "UTC",
  is_active: true,
  is_bot: false,
  is_email_verified: true,
  is_password_autoset: false,
  last_login_medium: "password",
  last_login_time: null,
};

/** The gate reads me, then profile, then workspaces (the sidebar warm). */
function queryClient(fixtures: { me?: unknown; profile?: unknown; workspaces?: unknown }): QueryClient {
  return {
    ensureQueryData: async (options: { queryKey: readonly unknown[] }) => {
      const key = options.queryKey[1];
      const value = fixtures[key as "me" | "profile" | "workspaces"];
      if (value === undefined) throw new Error(`no fixture for ${String(key)}`);
      if (value instanceof Error) throw value;
      return value;
    },
  } as unknown as QueryClient;
}

async function gate(fixtures: { me?: unknown; profile?: unknown; workspaces?: unknown }): Promise<unknown> {
  try {
    await enforceProfileSettingsGate({
      queryClient: queryClient(fixtures),
      apiClient: createClient({
        baseUrl: "https://api.test",
        transport: async () => {
          throw new Error("no fetch");
        },
      }),
      href: "/settings/profile/general/?tab=all",
    });
    return null;
  } catch (error) {
    return error;
  }
}

describe("enforceProfileSettingsGate", () => {
  it("lets a signed-in, onboarded member through", async () => {
    await expect(gate({ me: ME, profile: { is_onboarded: true }, workspaces: [] })).resolves.toBeNull();
  });

  it("bounces a visitor to the entry with the return path", async () => {
    const thrown = await gate({ me: new Error("signed out") });
    expect(isRedirect(thrown)).toBe(true);
    expect((thrown as { options: { to: string; search: unknown } }).options).toMatchObject({
      to: "/",
      search: { next_path: "/settings/profile/general/?tab=all" },
    });
  });

  it("continues unfinished members into onboarding", async () => {
    const thrown = await gate({ me: ME, profile: { is_onboarded: false } });
    expect(isRedirect(thrown)).toBe(true);
    expect((thrown as { options: { href: string } }).options.href).toBe("/onboarding");
  });
});
