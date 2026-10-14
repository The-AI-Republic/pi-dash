// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-445): instance-admin runners list. Row
// ADM-080 (runners list with provisioning/versions, filter,
// pagination).
//
// Observed behavior notes (paraphrased; the inventory row carries them
// at update time): this checkout has no Runners surface (bug:
// NEWFRONT-494) — the sidebar carries no Runners entry, the /runners/
// path falls through to the not-found screen with its way back to
// general settings, and the instance runners endpoint answers 404.
// Intended per the row: a Runners page listing every runner
// newest-first with provisioning, status, versions and heartbeat
// columns, a provisioning filter and pagination (unmerged on
// origin/pi-dash/pdashoss01-209 at 79f4241b).
import { test, expect } from "../fixtures";
import { serverAdminRunnersEndpointStatus, serverAdminSession } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";

test(
  specTitle(["ADM-080"], "bug: NEWFRONT-494 sidebar carries no Runners entry"),
  { tag: specTags(["ADM-080"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await driver.adminEnsureSignedIn(seed.email, seed.password);
    await driver.adminOpenPath("/workspace/");
    await driver.adminWaitForRootBranch("dashboard");
    const facts = await driver.adminRunnersFacts();
    expect(facts.sidebarEntryVisible).toBe(false);
    expect(facts.sidebarHref).toBe(null);
    const status = await serverAdminRunnersEndpointStatus(await serverAdminSession(seed.email, seed.password));
    expect(status).toBe(404);
  }
);

test(
  specTitle(["ADM-080"], "bug: NEWFRONT-494 /runners/ falls through to not-found"),
  { tag: specTags(["ADM-080"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await driver.adminEnsureSignedIn(seed.email, seed.password);
    await driver.adminOpenPath("/runners/");
    const facts = await driver.adminNotFoundFacts();
    expect(facts.illustrationVisible).toBe(true);
    expect(facts.messageVisible).toBe(true);
    expect(facts.actionLabel).toBe("Go to general settings");
    await driver.adminNotFoundFollowAction();
    expect(await driver.adminCurrentPath()).toBe("/god-mode/general/");
  }
);
