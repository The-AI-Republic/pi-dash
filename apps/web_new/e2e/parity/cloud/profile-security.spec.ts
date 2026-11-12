// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-440): hidden password security on cloud, row
// CLOUD-035 (password security settings are hidden on cloud), green on
// the cloud-edition old app first. The same file must go green on
// apps/web_new once the cloud profile area lands there.
//
// Observed behavior notes (cloud oracle): the profile sidebar omits the
// security entry while the instance reports itself cloud-managed, and a
// direct visit to the security URL forwards to the general tab — the
// forward passes through the shared shell's neutral loading render on
// the way (a transient single frame owned by the Profile inventory,
// asserted here by its outcome: the general tab active, no security
// content). Self-managed builds keep the entry and the tab; that half
// runs against a second, OSS-edition stack beside the cloud one (see
// "Cloud oracle" in stack/README.md): bring it up with its own
// namespace, then export PARITY_OSS_NS=<ns> before running.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { test, expect } from "../fixtures";
import type { ParitySeedFacts } from "../drivers/parity-driver";
import {
  cloudBrowserCookies,
  cloudInstanceRaw,
  sessionBrowserCookies,
  signInSession,
  WORKSPACE_ROLE_MEMBER,
} from "../helpers/api";
import { parityApiUrl, parityOracleUrl } from "../helpers/parity-env";
import { specTags, specTitle } from "../helpers/tags";
import { cloudWorkspaceUser } from "./cloud-user-setup";

/** Resolve the OSS stack's oracle URL, API URL, and seed facts from PARITY_OSS_NS. */
function ossStack(): { oracle: string; apiBase: string; seed: ParitySeedFacts } {
  const ns = (process.env["PARITY_OSS_NS"] ?? "").trim();
  if (ns === "") {
    throw new Error(
      "[parity] PARITY_OSS_NS is not set; bring up an OSS stack with its own namespace " +
        "(PARITY_NS=<ns> parity-up.sh, no PARITY_ORACLE_EDITION) and export PARITY_OSS_NS=<ns>."
    );
  }
  const env = { ...process.env, PARITY_NS: ns };
  delete env["PARITY_API_URL"];
  delete env["PARITY_ORACLE_URL"];
  delete env["PARITY_ORACLE_ADMIN_URL"];
  const seedFile = join(dirname(fileURLToPath(import.meta.url)), "..", `.seed-${ns.toLowerCase()}.json`);
  return {
    oracle: parityOracleUrl(env),
    apiBase: parityApiUrl(env),
    seed: JSON.parse(readFileSync(seedFile, "utf8")) as ParitySeedFacts,
  };
}

test(
  specTitle(["CLOUD-035"], "profile sidebar omits the security entry on cloud"),
  { tag: specTags(["CLOUD-035"]) },
  async ({ driver, seed }) => {
    // The account tabs are workspace pages on web_new (main#670), and the
    // cloud security guard keys off the instance either way, so both legs
    // enter through a member of the seed workspace at its general tab.
    const session = await cloudWorkspaceUser(seed, "parity-440-sec", WORKSPACE_ROLE_MEMBER, "Security Hidden");
    await driver.openAuthenticated(
      `/${seed.workspaceSlug}/settings/account/general/`,
      cloudBrowserCookies(session.cookie)
    );
    await expect.poll(() => driver.cloudProfileSidebarTabs(), { timeout: 60_000 }).toContain("AI Assistant");
    const tabs = await driver.cloudProfileSidebarTabs();
    expect(tabs).not.toContain("Security");
    expect(tabs).toContain("Profile");

    // Server state: the guard keys off the instance reporting cloud-managed.
    const instance = await cloudInstanceRaw(session.cookie);
    expect(instance.status).toBe(200);
    const config = instance.payload["config"] as Record<string, unknown>;
    expect(config["is_self_managed"]).toBe(false);
  }
);

test(
  specTitle(["CLOUD-035"], "direct security URL forwards to the general tab"),
  { tag: specTags(["CLOUD-035"]) },
  async ({ driver, seed }) => {
    const session = await cloudWorkspaceUser(seed, "parity-440-sec", WORKSPACE_ROLE_MEMBER, "Security Forward");
    await driver.openAuthenticated(
      `/${seed.workspaceSlug}/settings/account/security/`,
      cloudBrowserCookies(session.cookie)
    );
    // The target forwards the withheld tab to the account general tab inside
    // the reader's workspace (main#670); the old app keeps its standalone one.
    await expect
      .poll(() => driver.currentPath(), { timeout: 60_000 })
      .toBe(
        driver.target === "web_new" ? `/${seed.workspaceSlug}/settings/account/general/` : "/settings/profile/general/"
      );
    await expect.poll(() => driver.cloudProfileActiveTab(), { timeout: 60_000 }).toBe("general");
  }
);

test(
  specTitle(["CLOUD-035"], "self-managed builds keep the security tab"),
  { tag: specTags(["CLOUD-035"]) },
  async ({ driver }) => {
    const oss = ossStack();
    const cookie = await signInSession(oss.seed.email, oss.seed.password, oss.apiBase);
    await driver.page.context().clearCookies();
    // Single-arg form: both stacks share the localhost cookie domain.
    await driver.page.context().addCookies(sessionBrowserCookies(cookie));
    // This scenario deliberately reads the old app's DOM on both legs (the
    // OSS oracle keeps the security tab for self-managed builds), so the
    // address it navigates to and the landing it expects are the old app's
    // on both targets. Web_new's own security landing is pinned by the
    // direct-URL test above.
    await driver.page.goto(`${oss.oracle}/settings/profile/security`);
    await expect.poll(() => driver.currentPath(), { timeout: 60_000 }).toBe("/settings/profile/security");
    await expect.poll(() => driver.cloudProfileSidebarTabs(), { timeout: 60_000 }).toContain("AI Assistant");
    const tabs = await driver.cloudProfileSidebarTabs();
    expect(tabs).toContain("Security");
    await expect.poll(() => driver.cloudProfileActiveTab(), { timeout: 60_000 }).toBe("security");
  }
);
