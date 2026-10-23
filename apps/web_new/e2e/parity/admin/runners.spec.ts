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
// origin/pi-dash/pdashoss01-209 at 79f4241b). The new console builds
// the page to that intent (NEWFRONT-494/518); its backend endpoint is
// still missing on the base (PDASHOSS01-303), so the page renders the
// read it actually gets: the table when the endpoint answers, the
// failure state with its retry when it does not.
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
    if (driver.target === "web_new") {
      // Intended (NEWFRONT-494): the rail carries the runners area as
      // its last entry, routing to the console's runners page.
      expect(facts.sidebarEntryVisible).toBe(true);
      expect(facts.sidebarHref ?? "").toContain("/runners");
      return;
    }
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
    if (driver.target === "web_new") {
      // Intended (NEWFRONT-494): the address is a real page. The read
      // settles into the table (or its empty row) once the backend
      // serves the endpoint; while PDASHOSS01-303 is open it settles
      // into the failure state with its retry instead.
      await driver.adminOpenPath("/runners/");
      await driver.adminWaitForRootBranch("dashboard");
      const status = await serverAdminRunnersEndpointStatus(await serverAdminSession(seed.email, seed.password));
      expect([200, 404]).toContain(status);
      await test.step("the runners page holds the rail entry and one settled body", async () => {
        const facts = await driver.adminRunnersPageFacts();
        expect(facts.pageVisible).toBe(true);
        expect(facts.title).toBe("Runners");
        if (status === 200) {
          expect(facts.tableVisible).toBe(true);
          expect(facts.errorVisible).toBe(false);
        } else {
          expect(facts.errorVisible).toBe(true);
          expect(facts.tableVisible).toBe(false);
          expect(facts.retryVisible).toBe(true);
        }
      });
      return;
    }
    await driver.adminOpenPath("/runners/");
    const facts = await driver.adminNotFoundFacts();
    expect(facts.illustrationVisible).toBe(true);
    expect(facts.messageVisible).toBe(true);
    expect(facts.actionLabel).toBe("Go to general settings");
    await driver.adminNotFoundFollowAction();
    expect(await driver.adminCurrentPath()).toBe("/god-mode/general/");
  }
);
