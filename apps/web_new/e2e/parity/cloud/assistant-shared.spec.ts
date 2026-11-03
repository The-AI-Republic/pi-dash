// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-440): shared assistant-tab sections, row
// CLOUD-036 (the AI Assistant tab keeps shared sections reachable),
// green on the cloud-edition old app first. The same file must go green
// on apps/web_new once the cloud profile area lands there.
//
// Observed behavior notes (cloud oracle): the cloud page replaces the
// whole assistant tab, so it mounts the shared tool-server section
// itself — cloud users keep the ability to register their own servers —
// and points catalog browsers at the sidebar Apps section instead of
// duplicating the catalog. The catalog's own behavior belongs to
// NEWFRONT-439; here only the pointer and the sidebar entry it names.
import { test, expect } from "../fixtures";
import { cloudBrowserCookies } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { cloudOnboardedUser, cloudWorkspaceUser } from "./cloud-user-setup";

test(
  specTitle(["CLOUD-036"], "tool-server section renders inside the cloud page"),
  { tag: specTags(["CLOUD-036"]) },
  async ({ driver }) => {
    const session = await cloudOnboardedUser("parity-440-shared", "Shared Sections");
    await driver.openAuthenticated("/settings/profile/ai-assistant", cloudBrowserCookies(session.cookie));
    await expect.poll(() => driver.cloudAssistantView(), { timeout: 60_000 }).toBe("list");
    expect(await driver.cloudAssistantMcpSectionVisible()).toBe(true);
  }
);

test(
  specTitle(["CLOUD-036"], "apps pointer sends catalog browsers to the sidebar"),
  { tag: specTags(["CLOUD-036"]) },
  async ({ driver, seed }) => {
    const session = await cloudWorkspaceUser(seed, "parity-440-shared", 15, "Apps Pointer");
    await driver.openAuthenticated("/settings/profile/ai-assistant", cloudBrowserCookies(session.cookie));
    await expect.poll(() => driver.cloudAssistantView(), { timeout: 60_000 }).toBe("list");
    expect(await driver.cloudAssistantAppsPointerVisible()).toBe(true);

    // The named destination exists: the sidebar carries the Apps entry.
    await driver.openAuthenticated(
      `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`,
      cloudBrowserCookies(session.cookie)
    );
    await expect.poll(() => driver.sidebarEntryVisible("Apps"), { timeout: 60_000 }).toBe(true);
  }
);
