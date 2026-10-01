// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios: multi-select and bulk operations (NEWFRONT-120).
// Rows ISS-108–ISS-116. The seeded stack runs the OSS build, where bulk
// operations are unavailable: these scenarios pin the OSS-observable
// behavior. ISS-108–ISS-111 carry a bug marker (NEWFRONT-129): the
// inventory describes working selection mechanics plus an OSS upgrade
// banner, but selection is hard-disabled in this build, so no checkbox,
// no keyboard selection, no leave prompt, and no banner can ever appear.
import { test, expect } from "../fixtures";
import { serverIssueNames, signInSession } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

async function signInAndOpenIssues(
  driver: { openEntry(): Promise<void>; signInWithPasswordRetry(e: string, p: string): Promise<void> },
  seed: { email: string; password: string }
): Promise<void> {
  await driver.openEntry();
  await driver.signInWithPasswordRetry(seed.email, seed.password);
}

test(
  specTitle(["ISS-108"], "bug: NEWFRONT-129 list offers no selection checkboxes"),
  { tag: specTags(["ISS-108"]) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await signInAndOpenIssues(driver, seed);
    });

    await test.step("open the seeded project issues list", async () => {
      await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
      await driver.dismissWelcomeDialog();
    });

    await test.step("every seeded issue is listed", async () => {
      await expect
        .poll(() => driver.visibleIssueNames(), { timeout: 120_000 })
        .toEqual(expect.arrayContaining([...seed.issueNames]));
    });

    await test.step("no row or group selection checkbox is rendered", async () => {
      expect(await driver.selectionCheckboxCount()).toBe(0);
    });

    await test.step("the server agrees with the screen", async () => {
      const session = await signInSession(seed.email, seed.password);
      const server = await serverIssueNames(seed.workspaceSlug, seed.projectId, session);
      expect(server).toEqual(expect.arrayContaining([...seed.issueNames]));
    });
  }
);

test(
  specTitle(["ISS-109"], "bug: NEWFRONT-129 arrow keys drive no selection"),
  { tag: specTags(["ISS-109"]) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await signInAndOpenIssues(driver, seed);
    });

    await test.step("open the seeded project issues list", async () => {
      await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
      await driver.dismissWelcomeDialog();
      await expect
        .poll(() => driver.visibleIssueNames(), { timeout: 120_000 })
        .toEqual(expect.arrayContaining([...seed.issueNames]));
    });

    await test.step("arrow and shift-arrow keys select nothing", async () => {
      await driver.pressKey("ArrowDown");
      await driver.pressKey("ArrowDown");
      await driver.pressKey("ArrowDown", true);
      await driver.pressKey("ArrowUp", true);
      expect(await driver.selectionCheckboxCount()).toBe(0);
      expect(await driver.bulkBarVisible()).toBe(false);
    });
  }
);

test(
  specTitle(["ISS-110"], "bug: NEWFRONT-129 reload asks no leave confirmation"),
  { tag: specTags(["ISS-110"]) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await signInAndOpenIssues(driver, seed);
    });

    await test.step("open the seeded project issues list", async () => {
      await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
      await driver.dismissWelcomeDialog();
      await expect
        .poll(() => driver.visibleIssueNames(), { timeout: 120_000 })
        .toEqual(expect.arrayContaining([...seed.issueNames]));
    });

    await test.step("reloading shows no leave dialog and keeps the list", async () => {
      expect(await driver.reloadSawDialog()).toBe(false);
      await expect
        .poll(() => driver.visibleIssueNames(), { timeout: 120_000 })
        .toEqual(expect.arrayContaining([...seed.issueNames]));
    });
  }
);

test(
  specTitle(["ISS-111"], "bug: NEWFRONT-129 no upgrade banner renders"),
  { tag: specTags(["ISS-111"]) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await signInAndOpenIssues(driver, seed);
    });

    await test.step("open the seeded project issues list", async () => {
      await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
      await driver.dismissWelcomeDialog();
      await expect
        .poll(() => driver.visibleIssueNames(), { timeout: 120_000 })
        .toEqual(expect.arrayContaining([...seed.issueNames]));
    });

    await test.step("no bulk bar or upgrade banner is shown", async () => {
      expect(await driver.bulkBarVisible()).toBe(false);
    });
  }
);

test(
  specTitle(["ISS-116"], "gantt view exposes no bulk selection"),
  { tag: specTags(["ISS-116"]) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await signInAndOpenIssues(driver, seed);
    });

    await test.step("open the seeded project issues list", async () => {
      await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
      await driver.dismissWelcomeDialog();
      await expect
        .poll(() => driver.visibleIssueNames(), { timeout: 120_000 })
        .toEqual(expect.arrayContaining([...seed.issueNames]));
    });

    await test.step("the gantt view shows no selection controls", async () => {
      await driver.switchIssueLayout("Timeline Layout");
      await expect.poll(() => driver.pageTextContains("Today"), { timeout: 120_000 }).toBe(true);
      expect(await driver.selectionCheckboxCount()).toBe(0);
      expect(await driver.bulkBarVisible()).toBe(false);
    });

    await test.step("restore the list layout", async () => {
      await driver.switchIssueLayout("List Layout");
      await expect
        .poll(() => driver.visibleIssueNames(), { timeout: 120_000 })
        .toEqual(expect.arrayContaining([...seed.issueNames]));
    });
  }
);

test(
  specTitle(
    ["ISS-112", "ISS-113", "ISS-114", "ISS-115"],
    "cloud bulk update, delete, archive, subscribe stay unavailable"
  ),
  { tag: specTags(["ISS-112", "ISS-113", "ISS-114", "ISS-115"]) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await signInAndOpenIssues(driver, seed);
    });

    await test.step("open the seeded project issues list", async () => {
      await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
      await driver.dismissWelcomeDialog();
      await expect
        .poll(() => driver.visibleIssueNames(), { timeout: 120_000 })
        .toEqual(expect.arrayContaining([...seed.issueNames]));
    });

    await test.step("no bulk toolbar can act on the list", async () => {
      expect(await driver.selectionCheckboxCount()).toBe(0);
      expect(await driver.bulkBarVisible()).toBe(false);
    });

    await test.step("the server kept every seeded issue untouched", async () => {
      const session = await signInSession(seed.email, seed.password);
      const server = await serverIssueNames(seed.workspaceSlug, seed.projectId, session);
      expect(server).toEqual(expect.arrayContaining([...seed.issueNames]));
    });
  }
);
