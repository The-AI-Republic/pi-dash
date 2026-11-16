// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-437): downloads page, rows CLOUD-007
// (desktop installers per operating system) and CLOUD-008 (standalone
// CLI and web fallback), green on the cloud-edition old app first. The
// same file must go green on apps/web_new once the cloud marketing area
// lands there.
//
// Observed behavior notes (cloud oracle): each supported OS gets a card
// with an architecture note and a primary download through the stable
// latest-release URL; Windows and Linux add a secondary format while
// macOS ships one; a notice warns the installers are not yet
// publisher-signed. A separate CLI card offers the Windows installer on
// its own, explains the post-install sign-in step, links to the CLI
// setup guide, and points browser-preferring visitors at web sign-in.
import { test, expect } from "../fixtures";
import { specTags, specTitle } from "../helpers/tags";

const DESKTOP_LATEST = "https://pidash-desktop-releases.s3.us-west-2.amazonaws.com/downloads/latest";
const CLI_MSI = "https://github.com/The-AI-Republic/pi-dash/releases/latest/download/pidash-x86_64-pc-windows-msvc.msi";

test(
  specTitle(["CLOUD-007"], "each OS card offers its installers through the stable release URL"),
  { tag: specTags(["CLOUD-007"]) },
  async ({ driver }) => {
    await driver.openPath("/downloads");
    const cards = await driver.cloudDownloadCards();
    expect(cards.map((card) => card.os)).toEqual(["macOS", "Windows", "Linux"]);
    for (const card of cards) {
      expect(card.arch.length).toBeGreaterThan(0);
      expect(card.primaryHref.startsWith(`${DESKTOP_LATEST}/`)).toBe(true);
    }
    expect(cards[0]?.secondaryHref).toBeNull();
    expect(cards[0]?.primaryHref).toBe(`${DESKTOP_LATEST}/pidash-macos-universal.dmg`);
    expect(cards[1]?.primaryHref).toBe(`${DESKTOP_LATEST}/pidash-windows-x64-setup.exe`);
    expect(cards[1]?.secondaryHref).toBe(`${DESKTOP_LATEST}/pidash-windows-x64.msi`);
    expect(cards[2]?.primaryHref).toBe(`${DESKTOP_LATEST}/pidash-linux-x64.AppImage`);
    expect(cards[2]?.secondaryHref).toBe(`${DESKTOP_LATEST}/pidash-linux-x64.deb`);
    expect(await driver.cloudDownloadsSigningNoticeVisible()).toBe(true);
  }
);

test(
  specTitle(["CLOUD-008"], "CLI card covers the standalone installer and the web fallback"),
  { tag: specTags(["CLOUD-008"]) },
  async ({ driver }) => {
    await driver.openPath("/downloads");
    const cli = await driver.cloudCliCardFacts();
    expect(cli.installerHref).toBe(CLI_MSI);
    expect(cli.signinHintVisible).toBe(true);
    expect(cli.docsHref).toBe("/docs");

    await driver.openPath("/downloads");
    expect((await driver.cloudCliCardFacts()).webHref).toBe("/login");
  }
);
