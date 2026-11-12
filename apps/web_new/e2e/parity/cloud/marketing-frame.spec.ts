// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-437): shared marketing frame, row
// CLOUD-004 (header and footer on every public page), green on the
// cloud-edition old app first. The same file must go green on
// apps/web_new once the cloud marketing area lands there.
//
// Observed behavior notes (cloud oracle): the header carries the brand
// home link plus downloads, docs, pricing, repository, sign-in, and
// start actions; the footer repeats the section links with the
// repository and external terms/privacy. Below the small breakpoint the
// secondary header links hide (docs, pricing, start) with the repository
// link following at the medium breakpoint, while downloads and sign-in
// stay. Every header and footer link shows a keyboard focus ring.
import { test, expect } from "../fixtures";
import { specTags, specTitle } from "../helpers/tags";

const REPO_URL = "https://github.com/The-AI-Republic/pi-dash";
const PUBLIC_PATHS = ["/home", "/docs", "/downloads", "/pricing"];

test(
  specTitle(["CLOUD-004"], "header and footer repeat on every public page"),
  { tag: specTags(["CLOUD-004"]) },
  async ({ driver }) => {
    for (const path of PUBLIC_PATHS) {
      await driver.openPath(path);
      const header = await driver.cloudHeaderLinks();
      expect(header.map((link) => link.href)).toEqual([
        "/home",
        "/downloads",
        "/docs",
        "/pricing",
        REPO_URL,
        "/login",
        "/login",
      ]);
      expect(header.filter((link) => link.external).map((link) => link.href)).toEqual([REPO_URL]);
      expect(header.every((link) => link.visible)).toBe(true);

      const footer = await driver.cloudFooterLinks();
      expect(footer.map((link) => link.href)).toEqual([
        "/downloads",
        "/docs",
        "/pricing",
        REPO_URL,
        "https://airepublic.com/terms",
        "https://airepublic.com/privacy",
      ]);
      // Only the repository link opens a new tab; terms/privacy leave the
      // app in place.
      expect(footer.filter((link) => link.external).map((link) => link.href)).toEqual([REPO_URL]);
    }
  }
);

test(
  specTitle(["CLOUD-004"], "cramped widths hide secondary header links"),
  { tag: specTags(["CLOUD-004"]) },
  async ({ driver }) => {
    await driver.page.setViewportSize({ width: 500, height: 800 });
    try {
      await driver.openPath("/home");
      const header = await driver.cloudHeaderLinks();
      const byLabel = new Map(header.map((link) => [link.label, link]));
      expect(byLabel.get("Downloads")?.visible).toBe(true);
      expect(byLabel.get("Sign in")?.visible).toBe(true);
      expect(byLabel.get("Docs")?.visible).toBe(false);
      expect(byLabel.get("Pricing")?.visible).toBe(false);
      expect(byLabel.get("GitHub")?.visible).toBe(false);
      expect(byLabel.get("Start free")?.visible).toBe(false);
    } finally {
      await driver.page.setViewportSize({ width: 1280, height: 800 });
    }
  }
);

test(
  specTitle(["CLOUD-004"], "keyboard focus is visible on every frame link"),
  { tag: specTags(["CLOUD-004"]) },
  async ({ driver }) => {
    await driver.openPath("/pricing");
    expect(await driver.cloudHeaderLinksFocusVisible()).toBe(true);
    expect(await driver.cloudFooterLinksFocusVisible()).toBe(true);
  }
);
