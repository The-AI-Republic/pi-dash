// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Legacy settings addresses (parity/specs/profile.md §5, PROF-026;
// main#670): every workspace-less family's route resolves the claimed
// workspace through the gated reads and forwards into its account
// settings with the query kept; the workspace-owned families forward to
// their own workspace directly.
import type { QueryClient } from "@tanstack/react-query";
import { isNotFound, isRedirect } from "@tanstack/react-router";
import { createClient } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { Route as AccountCatchAll } from "../$ws/_gated/settings/account/$.js";
import { Route as AccountIndex } from "../$ws/_gated/settings/account/index.js";
import { Route as AccountTab } from "../$ws/_gated/settings/account/$tab.js";
import { Route as ApiTokens } from "../$ws/_gated/settings/api-tokens.js";
import { Route as Profile } from "../profile/$.js";
import { Route as SettingsIndex } from "./index.js";
import { Route as SettingsCatchAll } from "./$.js";
import { Route as SettingsProfile } from "./profile/$.js";

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

const WORKSPACES = [
  { id: "w1", name: "Acme", slug: "acme", logo_url: null, total_members: 1, role: 20 },
  { id: "w2", name: "Globex", slug: "globex", logo_url: null, total_members: 1, role: 20 },
];

/** The gated reads, keyed by the fixture name in the query key. */
function queryClient(fixtures: { last?: string | null; fallback?: string | null; workspaces?: unknown }): QueryClient {
  return {
    ensureQueryData: async (options: { queryKey: readonly unknown[] }) => {
      const key = options.queryKey[1];
      if (key === "me") return ME;
      if (key === "profile") return { is_onboarded: true };
      if (key === "workspaces") return fixtures.workspaces ?? WORKSPACES;
      if (key === "settings") {
        return {
          workspace: {
            last_workspace_slug: fixtures.last ?? "acme",
            fallback_workspace_slug: fixtures.fallback ?? null,
          },
        };
      }
      throw new Error(`no fixture for ${String(key)}`);
    },
  } as unknown as QueryClient;
}

function apiClient() {
  return createClient({
    baseUrl: "https://api.test",
    transport: async () => {
      throw new Error("no fetch");
    },
  });
}

async function thrown(run: () => unknown): Promise<unknown> {
  try {
    await run();
  } catch (error) {
    return error;
  }
  return null;
}

async function forwardOf(run: () => unknown): Promise<string> {
  const error = await thrown(run);
  expect(isRedirect(error)).toBe(true);
  return (error as { options: { href: string } }).options.href;
}

function context(fixtures: { last?: string | null; fallback?: string | null; workspaces?: unknown } = {}) {
  return { queryClient: queryClient(fixtures), apiClient: apiClient() } as never;
}

/**
 * Every forwarding route in one table, each entry carrying both a query and
 * a fragment. The cases below prove individual decisions; this table is the
 * whole set, so a route that starts dropping the query or the fragment (the
 * defect that came back three review rounds running) fails here once.
 */
const FORWARD_TABLE: Array<{
  name: string;
  route: { options: { beforeLoad?: unknown } };
  extra: Record<string, unknown>;
  pathname: string;
  searchStr: string;
  hash: string;
  expected: string;
}> = [
  {
    name: "/settings",
    route: SettingsIndex,
    extra: { context: context({ last: "acme" }) },
    pathname: "/settings",
    searchStr: "?keep=1",
    hash: "#top",
    expected: "/acme/settings/account/general/?keep=1#top",
  },
  {
    name: "/settings/<leftover>",
    route: SettingsCatchAll,
    extra: { context: context({ last: "acme" }) },
    pathname: "/settings/account/security",
    searchStr: "?keep=1",
    hash: "#top",
    expected: "/acme/settings/account/general/?keep=1#top",
  },
  {
    name: "/settings/profile/<tab>",
    route: SettingsProfile,
    extra: { context: context({ last: "acme" }) },
    pathname: "/settings/profile/ai-assistant",
    searchStr: "?keep=1",
    hash: "#voice-dictation",
    expected: "/acme/settings/account/ai-assistant/?keep=1#voice-dictation",
  },
  {
    name: "/profile/<tab>",
    route: Profile,
    extra: { context: context({ last: "acme" }) },
    pathname: "/profile/preferences",
    searchStr: "?keep=1",
    hash: "#theme",
    expected: "/acme/settings/account/preferences/?keep=1#theme",
  },
  {
    name: "/<ws>/settings/account (bare)",
    route: AccountIndex,
    extra: { params: { ws: "acme" } },
    pathname: "/acme/settings/account",
    searchStr: "?keep=1",
    hash: "#top",
    expected: "/acme/settings/account/general/?keep=1#top",
  },
  {
    name: "/<ws>/settings/account/<deep>",
    route: AccountCatchAll,
    extra: { params: { ws: "acme" } },
    pathname: "/acme/settings/account/security/extra",
    searchStr: "?keep=1",
    hash: "#top",
    expected: "/acme/settings/account/general/?keep=1#top",
  },
  {
    name: "/<ws>/settings/account/<unknown tab>",
    route: AccountTab,
    extra: { params: { ws: "acme", tab: "bogus" }, context: { queryClient: queryClient({}), apiClient: apiClient() } },
    pathname: "/acme/settings/account/bogus/",
    searchStr: "?keep=1",
    hash: "#top",
    expected: "/acme/settings/account/general/?keep=1#top",
  },
  {
    name: "/<ws>/settings/api-tokens",
    route: ApiTokens,
    extra: { params: { ws: "acme" } },
    pathname: "/acme/settings/api-tokens",
    searchStr: "?keep=1",
    hash: "#tokens",
    expected: "/acme/settings/account/api-tokens/?keep=1#tokens",
  },
];

describe("every forward carries the query and the fragment", () => {
  for (const entry of FORWARD_TABLE) {
    it(`lands ${entry.name} with both`, async () => {
      const href = await forwardOf(() =>
        (entry.route.options.beforeLoad as (args: unknown) => unknown)({
          ...entry.extra,
          location: { pathname: entry.pathname, searchStr: entry.searchStr, hash: entry.hash },
        })
      );
      expect(href).toBe(entry.expected);
    });
  }
});

describe("legacy settings forwards", () => {
  it("sends the bare settings address into the current workspace's general tab", async () => {
    expect(
      await forwardOf(() =>
        SettingsIndex.options.beforeLoad?.({
          context: context({ last: "globex" }),
          location: { pathname: "/settings/", searchStr: "?keep=1" },
        } as never)
      )
    ).toBe("/globex/settings/account/general/?keep=1");
  });

  it("forwards both profile-settings families to the workspace account tab", async () => {
    expect(
      await forwardOf(() =>
        SettingsProfile.options.beforeLoad?.({
          context: context({ last: "acme" }),
          location: { pathname: "/settings/profile/security/", searchStr: "" },
        } as never)
      )
    ).toBe("/acme/settings/account/security/");
    expect(
      await forwardOf(() =>
        Profile.options.beforeLoad?.({
          context: context({ last: "acme" }),
          location: { pathname: "/profile/preferences", searchStr: "?x=1" },
        } as never)
      )
    ).toBe("/acme/settings/account/preferences/?x=1");
  });

  it("answers a deeper profile address with the not-found page", async () => {
    const error = await thrown(() =>
      Profile.options.beforeLoad?.({
        context: context(),
        location: { pathname: "/profile/security/extra", searchStr: "" },
      } as never)
    );
    expect(isNotFound(error)).toBe(true);
  });

  it("maps the workspace tokens address to the account tokens tab with its query", async () => {
    expect(
      await forwardOf(() =>
        ApiTokens.options.beforeLoad?.({
          params: { ws: "acme" },
          location: { pathname: "/acme/settings/api-tokens/", searchStr: "?page=2" },
        } as never)
      )
    ).toBe("/acme/settings/account/api-tokens/?page=2");
  });

  it("sends a deeper workspace account address to that workspace's general tab", async () => {
    expect(
      await forwardOf(() =>
        AccountCatchAll.options.beforeLoad?.({
          params: { ws: "acme" },
          location: { pathname: "/acme/settings/account/security/extra", searchStr: "?a=1" },
        } as never)
      )
    ).toBe("/acme/settings/account/general/?a=1");
  });

  it("forwards every other old /settings address through the catch-all to the general tab", async () => {
    // The old app's `settings/*` splat kept these working; only a deeper
    // `/settings/profile/...` is a family of its own, and it lands on
    // general too.
    for (const pathname of ["/settings/account/security", "/settings/bogus", "/settings/profile/a/b"]) {
      expect(
        await forwardOf(() =>
          SettingsCatchAll.options.beforeLoad?.({
            context: context({ last: "globex" }),
            location: { pathname, searchStr: "?x=1", hash: "" },
          } as never)
        )
      ).toBe("/globex/settings/account/general/?x=1");
    }
  });

  it("carries the fragment of a bookmarked staff page address", async () => {
    expect(
      await forwardOf(() =>
        SettingsProfile.options.beforeLoad?.({
          context: context({ last: "acme" }),
          location: { pathname: "/settings/profile/ai-assistant", searchStr: "", hash: "voice-dictation" },
        } as never)
      )
    ).toBe("/acme/settings/account/ai-assistant/#voice-dictation");
  });

  it("sends a reader who belongs nowhere to workspace creation", async () => {
    expect(
      await forwardOf(() =>
        SettingsIndex.options.beforeLoad?.({
          context: context({ last: null, fallback: null, workspaces: [] }),
          location: { pathname: "/settings/", searchStr: "" },
        } as never)
      )
    ).toBe("/create-workspace");
  });
});
