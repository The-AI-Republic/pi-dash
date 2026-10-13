// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-437): docs page, row CLOUD-006 (the page
// teaches the zero-to-runner path), green on the cloud-edition old app
// first. The same file must go green on apps/web_new once the cloud
// marketing area lands there.
//
// Observed behavior notes (cloud oracle): the page opens with a sign-in
// entry and a repository link, explains the three-part model (cloud,
// CLI, runner), then walks four quick-start steps — project creation,
// per-OS CLI install commands (plus a Windows MSI link for managed
// deployments), device login, runner enrollment — with the optional
// agent skill command and a closing trust band. Assertions stay
// structural (counts and link targets) and never quote the page copy.
import { test, expect } from "../fixtures";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["CLOUD-006"], "docs page walks the zero-to-runner path"),
  { tag: specTags(["CLOUD-006"]) },
  async ({ driver }) => {
    await driver.openPath("/docs");
    const facts = await driver.cloudDocsFacts();
    expect(facts.signInHref).toBe("/login");
    expect(facts.repoHref).toBe("https://github.com/The-AI-Republic/pi-dash");
    expect(facts.concepts).toBe(3);
    expect(facts.steps).toBe(4);
    // Two install commands, device login, runner enrollment, agent skill.
    expect(facts.codeBlocks).toBe(5);
    expect(facts.installRefsReleaseDownload).toBe(true);
    expect(facts.msiHref).toBe(
      "https://github.com/The-AI-Republic/pi-dash/releases/latest/download/pidash-x86_64-pc-windows-msvc.msi"
    );
    expect(facts.footerItems).toBe(3);
  }
);
