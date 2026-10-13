// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-437): marketing home, row CLOUD-003 (the
// page presents the product and routes visitors onward), green on the
// cloud-edition old app first. The same file must go green on
// apps/web_new once the cloud marketing area lands there.
//
// Observed behavior notes (cloud oracle): the hero offers three entry
// actions (sign-in, downloads, docs); below it a value strip, a
// four-step delivery workflow, four capability articles, an open-source
// panel linking out to the public repository, and a closing call to
// action back to sign-in. Assertions stay structural (counts and link
// targets) and never quote the page copy.
import { test, expect } from "../fixtures";
import { specTags, specTitle } from "../helpers/tags";

const REPO_URL = "https://github.com/The-AI-Republic/pi-dash";

test(
  specTitle(["CLOUD-003"], "hero entry actions reach sign-in, downloads, and docs"),
  { tag: specTags(["CLOUD-003"]) },
  async ({ driver }) => {
    await driver.openPath("/home");
    expect(await driver.cloudHomeHeroActionHrefs()).toEqual(["/login", "/downloads", "/docs"]);

    await driver.cloudHomeClickHeroAction("/downloads");
    await expect.poll(() => driver.currentPath(), { timeout: 60_000 }).toBe("/downloads");

    await driver.openPath("/home");
    await driver.cloudHomeClickHeroAction("/docs");
    await expect.poll(() => driver.currentPath(), { timeout: 60_000 }).toBe("/docs");

    await driver.openPath("/home");
    await driver.cloudHomeClickHeroAction("/login");
    await expect.poll(() => driver.currentPath(), { timeout: 60_000 }).toBe("/login");
  }
);

test(
  specTitle(["CLOUD-003"], "home sections follow the hero in order"),
  { tag: specTags(["CLOUD-003"]) },
  async ({ driver }) => {
    await driver.openPath("/home");
    const sections = await driver.cloudHomeSections();
    expect(sections.stripItems).toBe(4);
    expect(sections.workflowSteps).toBe(4);
    expect(sections.capabilityCards).toBe(4);
    expect(sections.repoHref).toBe(REPO_URL);
    expect(sections.repoExternal).toBe(true);
    expect(sections.closingCtaHref).toBe("/login");
  }
);
