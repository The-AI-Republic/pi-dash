// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenario (NEWFRONT-112): every comment operation resolves in
// exactly one visible notice of the matching kind — success for create,
// update and delete, and an error notice when the create request fails.
// Row: CMT-010.
import { test, expect } from "../fixtures";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["CMT-010"];

// Cold dev servers compile route bundles on first load, which can take
// minutes on a loaded host; each step still carries its own poll timeout.
test.setTimeout(600_000);

test(
  specTitle(ROWS, "success and failure notices for comment operations"),
  { tag: specTags(ROWS) },
  async ({ driver, seed, page }) => {
    const marker = `composer notice ${Date.now()}`;
    await test.step("sign in and open the work item", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.composerOpenIssue(seed.workspaceSlug, "PAR-1");
    });

    // Toasts dismiss within seconds, so a notice must be caught while the
    // operation runs — a snapshot taken after the step's slower render
    // reads would outlive it. Samples the notice region during the action
    // and returns the newly seen notices (anything already visible before
    // the action, e.g. a lingering toast from the previous step, is
    // excluded by message).
    async function noticesAddedDuring(action: () => Promise<void>): Promise<{ message: string; kind: string }[]> {
      const before = new Set((await driver.composerVisibleNotices()).map((notice) => notice.message));
      const seen = new Map<string, { message: string; kind: string }>();
      let stop = false;
      const sampler = (async () => {
        while (!stop) {
          const current = await driver.composerVisibleNotices();
          for (const notice of current) {
            if (!seen.has(notice.message)) seen.set(notice.message, notice);
          }
          await page.waitForTimeout(200);
        }
      })();
      await action();
      // Let a trailing toast land after the action's last render read.
      await page.waitForTimeout(3_000);
      stop = true;
      await sampler;
      const added = [...seen.values()].filter((notice) => !before.has(notice.message));
      // The operation must have produced its notice while watched: an empty
      // result means the toast never fired, not that the sampler was slow.
      expect(added.length).toBeGreaterThan(0);
      return added;
    }

    await test.step("create resolves in one success notice", async () => {
      const added = await noticesAddedDuring(async () => {
        await driver.composerType(marker);
        await driver.composerSubmit();
        await expect
          .poll(() => driver.composerVisibleCommentTexts(), { timeout: 60_000 })
          .toEqual(expect.arrayContaining([expect.stringContaining(marker)]));
      });
      expect(added).toHaveLength(1);
      expect(added[0]).toMatchObject({ kind: "success" });
    });

    await test.step("update resolves in one success notice", async () => {
      const added = await noticesAddedDuring(async () => {
        await driver.composerOpenCommentMenu(marker);
        await driver.composerMenuClick("Edit");
        await driver.composerEditType(`${marker} revised`);
        await driver.composerEditSave();
        await expect
          .poll(() => driver.composerVisibleCommentTexts(), { timeout: 60_000 })
          .toEqual(expect.arrayContaining([expect.stringContaining(`${marker} revised`)]));
      });
      expect(added).toHaveLength(1);
      expect(added[0]).toMatchObject({ kind: "success" });
    });

    await test.step("a failed create surfaces one error notice", async () => {
      await page.route("**/api/workspaces/*/projects/*/issues/*/comments/", (route) => route.abort(), {
        times: 1,
      });
      const failed = `composer notice failure ${Date.now()}`;
      const added = await noticesAddedDuring(async () => {
        await driver.composerType(failed);
        await driver.composerSubmit();
      });
      expect(added).toHaveLength(1);
      expect(added[0]).toMatchObject({ kind: "error" });
      // The failed post also clears the composer, matching the successful
      // submit behavior — record the oracle outcome, not the draft.
      expect(await driver.composerDraftText()).toBe("");
      await expect
        .poll(() => driver.composerVisibleCommentTexts(), { timeout: 30_000 })
        .not.toEqual(expect.arrayContaining([expect.stringContaining(failed)]));
      await page.unrouteAll({ behavior: "wait" });
    });

    await test.step("delete resolves in one success notice", async () => {
      const added = await noticesAddedDuring(async () => {
        await driver.composerOpenCommentMenu(`${marker} revised`);
        await driver.composerMenuClick("Delete");
        await expect
          .poll(() => driver.composerVisibleCommentTexts(), { timeout: 60_000 })
          .not.toEqual(expect.arrayContaining([expect.stringContaining(marker)]));
      });
      expect(added).toHaveLength(1);
      expect(added[0]).toMatchObject({ kind: "success" });
    });
  }
);
