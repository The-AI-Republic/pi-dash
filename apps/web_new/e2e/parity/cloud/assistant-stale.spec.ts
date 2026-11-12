// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-440): stale hosted-model lane, row
// CLOUD-034 (the lane degrades gracefully on stale sessions), green on
// the cloud-edition old app first. The same file must go green on
// apps/web_new once the cloud profile area lands there.
//
// Observed behavior notes (cloud oracle): the connections read flags
// gateway availability, and a session that predates the gateway scopes
// gets a notice on the affected managed rows directing sign-out-and-
// back-in — a refresh alone cannot acquire the scopes. Where the lane
// itself is release-gated off, no managed row renders at all rather
// than a lane that cannot work; this stack gates it off, so the notice
// branch runs against the stubbed server shape a stale session
// produces (a managed row with the flag down).
import { test, expect } from "../fixtures";
import { cloudAssistantConnectionsRaw, cloudBrowserCookies } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { cloudOnboardedUser } from "./cloud-user-setup";

const CONNECTIONS_PATH = "**/api/users/me/ai-assistant/connections/";

test(
  specTitle(["CLOUD-034"], "gated-off lane offers no managed row that cannot work"),
  { tag: specTags(["CLOUD-034"]) },
  async ({ driver }) => {
    const session = await cloudOnboardedUser("parity-440-stale", "Stale Lane");
    await driver.openAuthenticated("/settings/profile/ai-assistant", cloudBrowserCookies(session.cookie));
    await expect.poll(() => driver.cloudAssistantConnections(), { timeout: 60_000 }).not.toHaveLength(0);
    const titles = (await driver.cloudAssistantConnections()).map((card) => card.title);
    expect(titles).not.toContain("OpenHub");

    // Server state: the flag reads down with no managed options or rows.
    const list = await cloudAssistantConnectionsRaw(session.cookie);
    expect(list.status).toBe(200);
    expect(list.payload["openhub_available"]).toBe(false);
    expect(list.payload["openhub_model_options"]).toEqual([]);
    const rows = (list.payload["connections"] as { kind: string }[]) ?? [];
    expect(rows.some((row) => row.kind === "openhub")).toBe(false);
  }
);

test(
  specTitle(["CLOUD-034"], "stale sessions carry a sign-out-and-back-in notice"),
  { tag: specTags(["CLOUD-034"]) },
  async ({ driver }) => {
    const session = await cloudOnboardedUser("parity-440-stale", "Stale Notice");
    const live = await cloudAssistantConnectionsRaw(session.cookie);
    expect(live.status).toBe(200);
    const base = live.payload as Record<string, unknown>;
    const stale = {
      ...base,
      connections: [
        ...((base["connections"] as Record<string, unknown>[]) ?? []),
        {
          id: "00000000-0000-4000-8000-000000000000",
          kind: "openhub",
          provider_kind: "",
          base_url: "",
          model_name: "moonshotai/kimi-k3",
          has_api_key: false,
          is_selected: false,
          last_verified_at: null,
          created_at: new Date().toISOString(),
          connected: true,
          uses_chatgpt_plan: false,
        },
      ],
      openhub_available: false,
    };
    await driver.page.route(CONNECTIONS_PATH, async (route) => {
      const request = route.request();
      if (request.method() !== "GET" || new URL(request.url()).pathname !== "/api/users/me/ai-assistant/connections/") {
        await route.fallback();
        return;
      }
      await route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(stale) });
    });
    await driver.openAuthenticated("/settings/profile/ai-assistant", cloudBrowserCookies(session.cookie));
    await expect
      .poll(() => driver.cloudAssistantConnections(), { timeout: 60_000 })
      .toEqual(expect.arrayContaining([expect.objectContaining({ title: "OpenHub" })]));
    const notice = await driver.cloudAssistantStaleNotice("OpenHub");
    expect(notice).not.toBeNull();
    expect(notice ?? "").toContain("Sign out and back in");
    await driver.page.unroute(CONNECTIONS_PATH);
  }
);
