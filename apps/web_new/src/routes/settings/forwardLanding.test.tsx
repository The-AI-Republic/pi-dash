// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The legacy forwards as the real router completes them (parity/specs/
// profile.md §5, PROF-026; main#670). The route modules decide the
// redirect, but the router then commits it with `trailingSlash: "preserve"`
// (main.tsx) and keeps the query and fragment. These cases drive the
// production tree so a forward's landing — not only its target string —
// is pinned: a slashed address stays slashed, `?x=1` survives, and a
// bookmarked `#voice-dictation` anchors the AI Assistant tab.
import { QueryClient } from "@tanstack/react-query";
import { createClient, type Transport } from "@pidash/api-client";
import { createMemoryHistory, createRouter } from "@tanstack/react-router";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { routeTree } from "../../routeTree.gen.js";

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

/** A contract-valid profile whose onboarded marker lets the gate pass. */
const PROFILE = {
  id: "223e4567-e89b-12d3-a456-426614174000",
  user: ME.id,
  theme: {},
  is_app_rail_docked: false,
  is_tour_completed: true,
  onboarding_step: {},
  use_case: null,
  role: null,
  is_onboarded: true,
  last_workspace_id: null,
  billing_address_country: "US",
  billing_address: null,
  has_billing_address: false,
  company_name: "",
  notification_view_mode: "all",
  is_smooth_cursor_enabled: false,
  is_mobile_onboarded: true,
  mobile_onboarding_step: {},
  mobile_timezone_auto_set: false,
  language: "en",
  start_of_the_week: 0,
  goals: {},
  background_color: "",
  is_navigation_tour_completed: true,
  has_marketing_email_consent: false,
  is_subscribed_to_changelog: false,
  product_tour: {},
  settings: {},
  created_at: "2024-01-01T00:00:00Z",
  updated_at: "2024-01-01T00:00:00Z",
};

const ACME_ID = "323e4567-e89b-12d3-a456-426614174000";
const GLOBEX_ID = "423e4567-e89b-12d3-a456-426614174000";

const WORKSPACES = [
  { id: ACME_ID, name: "Acme", slug: "acme", logo_url: null, total_members: 1, role: 20 },
  { id: GLOBEX_ID, name: "Globex", slug: "globex", logo_url: null, total_members: 1, role: 20 },
];

function reply(body: unknown): Awaited<ReturnType<Transport>> {
  return {
    status: 200,
    url: "",
    headers: { get: () => "application/json" },
    text: async () => JSON.stringify(body),
  };
}

/** The caller's own membership in the workspace the request names, or null. */
function membershipFor(url: string): unknown {
  const slug = /\/workspaces\/([^/]+)\/workspace-members\/me\//.exec(url)?.[1] ?? "";
  const workspace = WORKSPACES.find((entry) => entry.slug === slug);
  if (workspace === undefined) return null;
  return {
    id: workspace.id.replace("323e", "523e").replace("423e", "623e"),
    workspace: workspace.id,
    member: ME.id,
    role: workspace.role,
    company_role: null,
    view_props: {},
    default_props: {},
    issue_props: {},
    is_active: true,
    getting_started_checklist: {},
    tips: {},
    explored_features: {},
    draft_issue_count: 0,
    created_by: null,
    updated_by: null,
    created_at: "2024-01-01T00:00:00Z",
    updated_at: "2024-01-01T00:00:00Z",
  };
}

/** Everything the signed-in workspace tree reads; any other request 404s. */
const transport: Transport = async (url) => {
  if (url.includes("/users/me/settings/")) {
    return reply({
      id: ME.id,
      email: ME.email,
      workspace: {
        last_workspace_id: ACME_ID,
        last_workspace_slug: "acme",
        fallback_workspace_id: null,
        fallback_workspace_slug: null,
        invites: 0,
      },
    });
  }
  if (url.includes("/users/me/profile/")) return reply(PROFILE);
  if (url.includes("/users/me/workspaces/")) return reply(WORKSPACES);
  if (url.includes("/workspace-members/me/")) return reply(membershipFor(url));
  if (/\/users\/me\/?$/.test(url)) return reply(ME);
  return { status: 404, url, headers: { get: () => "application/json" }, text: async () => "{}" };
};

/**
 * Load the production tree from one legacy address and report where the
 * router settled: pathname, query and fragment as one string.
 */
async function landing(entry: string): Promise<string> {
  const apiClient = createClient({ baseUrl: "https://api.test", transport });
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const router = createRouter({
    routeTree,
    history: createMemoryHistory({ initialEntries: [entry] }),
    context: { queryClient, apiClient },
    // The app's own setting (main.tsx); a test that dropped it would pass
    // on a slash-stripping target address the browser never shows.
    trailingSlash: "preserve",
  });
  await router.load();
  const settled = entry.split(/[?#]/)[0] ?? entry;
  for (let tick = 0; tick < 200 && router.history.location.pathname === settled; tick += 1) {
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  const location = router.history.location;
  return `${location.pathname}${location.search}${location.hash}`;
}

beforeEach(() => {
  vi.stubGlobal("scrollTo", vi.fn());
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("legacy settings forwards as the router lands them", () => {
  it("lands the bare settings addresses on the general tab with the query kept", async () => {
    expect(await landing("/settings")).toBe("/acme/settings/account/general/");
    expect(await landing("/settings?x=1")).toBe("/acme/settings/account/general/?x=1");
    expect(await landing("/settings/profile/")).toBe("/acme/settings/account/general/");
  });

  it("lands every other old /settings address on the general tab, not the not-found page", async () => {
    // The old app's `settings/*` splat sent stray leftovers to a real page.
    expect(await landing("/settings/account/security")).toBe("/acme/settings/account/general/");
    expect(await landing("/settings/bogus?x=1")).toBe("/acme/settings/account/general/?x=1");
  });

  it("lands the legacy profile family on the named tab", async () => {
    expect(await landing("/settings/profile/security?keep=2")).toBe("/acme/settings/account/security/?keep=2");
    expect(await landing("/profile/preferences?keep=4")).toBe("/acme/settings/account/preferences/?keep=4");
    expect(await landing("/profile/")).toBe("/acme/settings/account/general/");
  });

  it("carries a bookmarked fragment onto the tab", async () => {
    expect(await landing("/settings/profile/ai-assistant#voice-dictation")).toBe(
      "/acme/settings/account/ai-assistant/#voice-dictation"
    );
  });

  it("lands the workspace addresses inside their own workspace", async () => {
    // A tab address that needs no forward keeps the slash form it was typed
    // with (`trailingSlash: "preserve"`), and keeps its query.
    expect(await landing("/acme/settings/account/security?keep=2")).toBe("/acme/settings/account/security?keep=2");
    expect(await landing("/acme/settings/account/security/?keep=2")).toBe("/acme/settings/account/security/?keep=2");
    // The two addresses that do forward land on the slashed tab the app
    // itself links to, in their own workspace.
    expect(await landing("/acme/settings/account/")).toBe("/acme/settings/account/general/");
    expect(await landing("/acme/settings/api-tokens")).toBe("/acme/settings/account/api-tokens/");
  });

  it("answers the not-found page for a deeper member address", async () => {
    // `/profile/<id>/<tab>` is a member address, outside every family.
    expect(await landing("/profile/a/b")).toBe("/profile/a/b");
  });
});
