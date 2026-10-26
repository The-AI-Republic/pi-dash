// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios: multi-select and bulk operations (NEWFRONT-120).
// Rows ISS-108–ISS-116. The seeded stack runs the OSS build, where bulk
// operations are unavailable. OSS selection is a documented old-app bug
// (NEWFRONT-129): the old app hard-disables it, while apps/web_new owns
// the intended mechanics plus the OSS upgrade banner. The ISS-108–111
// scenarios branch on their target so the oracle pins the old absence and
// the new app proves the intended behavior.
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

/** Tick the first row's selection control on the new app (ISS-108). */
async function selectFirstRow(driver: { page: import("@playwright/test").Page }): Promise<void> {
  const first = driver.page.getByTestId("selection-checkbox").first();
  await first.waitFor({ state: "visible", timeout: 120_000 });
  await first.click();
}

test(
  specTitle(["ISS-108"], "bug: NEWFRONT-129 list selection follows the target"),
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

    if (driver.target === "web_new") {
      await test.step("rows and groups offer selection controls", async () => {
        expect(await driver.selectionCheckboxCount()).toBeGreaterThan(0);
      });
      await test.step("a row toggle raises the OSS upgrade banner", async () => {
        await driver.selectFirstRow();
        expect(await driver.bulkBarVisible()).toBe(true);
      });
    } else {
      await test.step("the old app renders no selection controls", async () => {
        expect(await driver.selectionCheckboxCount()).toBe(0);
      });
    }

    await test.step("the server agrees with the screen", async () => {
      const session = await signInSession(seed.email, seed.password);
      const server = await serverIssueNames(seed.workspaceSlug, seed.projectId, session);
      expect(server).toEqual(expect.arrayContaining([...seed.issueNames]));
    });
  }
);

test(
  specTitle(["ISS-109"], "bug: NEWFRONT-129 keyboard selection follows the target"),
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

    await test.step("arrow and shift-arrow keys follow the target", async () => {
      await driver.pressKey("ArrowDown");
      await driver.pressKey("ArrowDown");
      await driver.pressKey("ArrowDown", true);
      await driver.pressKey("ArrowUp", true);
      if (driver.target === "web_new") {
        expect(await driver.bulkBarVisible()).toBe(true);
      } else {
        expect(await driver.selectionCheckboxCount()).toBe(0);
        expect(await driver.bulkBarVisible()).toBe(false);
      }
    });
  }
);

test(
  specTitle(["ISS-110"], "bug: NEWFRONT-129 leave confirmation follows the target"),
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

    if (driver.target === "web_new") {
      await test.step("selecting a row arms the reload confirmation", async () => {
        await driver.selectFirstRow();
        expect(await driver.reloadSawDialog()).toBe(true);
      });
    } else {
      await test.step("reloading shows no leave dialog", async () => {
        expect(await driver.reloadSawDialog()).toBe(false);
      });
    }
    await test.step("the list survives the reload", async () => {
      await expect
        .poll(() => driver.visibleIssueNames(), { timeout: 120_000 })
        .toEqual(expect.arrayContaining([...seed.issueNames]));
    });
  }
);

test(
  specTitle(["ISS-111"], "bug: NEWFRONT-129 OSS upgrade banner follows the target"),
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

    await test.step("the bulk bar follows the target", async () => {
      if (driver.target === "web_new") {
        expect(await driver.bulkBarVisible()).toBe(false);
        await driver.selectFirstRow();
        expect(await driver.bulkBarVisible()).toBe(true);
      } else {
        expect(await driver.bulkBarVisible()).toBe(false);
      }
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
      if (driver.target === "web_new") {
        // OSS selection exists on the new app (ISS-108); only the
        // functional toolbar is cloud-gated, so with nothing selected
        // neither app shows a bar.
        expect(await driver.selectionCheckboxCount()).toBeGreaterThan(0);
      } else {
        expect(await driver.selectionCheckboxCount()).toBe(0);
      }
      expect(await driver.bulkBarVisible()).toBe(false);
    });

    await test.step("the server kept every seeded issue untouched", async () => {
      const session = await signInSession(seed.email, seed.password);
      const server = await serverIssueNames(seed.workspaceSlug, seed.projectId, session);
      expect(server).toEqual(expect.arrayContaining([...seed.issueNames]));
    });
  }
);
