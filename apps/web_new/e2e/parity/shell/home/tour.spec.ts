// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenario (NEWFRONT-123): first-run tour overlay. Row: SHELL-006.
// Behavior learned from the old dashboard in prose: while the profile
// flag is unset, home renders a full-screen tour card above the content;
// the welcome card offers starting the tour or declining it, each step
// offers Back/Next, and the last step finishes into project creation.
// Every exit stores the flag, so later visits show the dashboard.
import { test, expect } from "../../fixtures";
import { serverSetTourCompleted, serverTourCompleted, signInSessionRetry } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-006"];

test(
  specTitle(ROWS, "first-run tour covers home until completed"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const session = await signInSessionRetry(seed.email, seed.password);

    await test.step("sign in and open home with an incomplete tour", async () => {
      await serverSetTourCompleted(session, false);
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.homeOpen(seed.workspaceSlug);
      expect(await driver.homeTourVisible()).toBe(true);
    });

    await test.step("declining the tour reveals the dashboard and persists", async () => {
      await driver.homeTourDismiss();
      await expect.poll(() => driver.homeTourVisible(), { timeout: 30_000 }).toBe(false);
      await expect.poll(() => serverTourCompleted(session), { timeout: 30_000 }).toBe(true);
      await driver.homeReload();
      expect(await driver.homeTourVisible()).toBe(false);
    });

    await test.step("walking the full tour also completes it", async () => {
      // The flag write and the tour render each cross the scratch stack,
      // so a loaded moment can drop either side of the round; confirm the
      // write, then retry the round a few times before failing honestly.
      let shown = false;
      for (let round = 0; round < 3 && !shown; round += 1) {
        await serverSetTourCompleted(session, false);
        await expect.poll(() => serverTourCompleted(session), { timeout: 30_000 }).toBe(false);
        await driver.homeReload();
        try {
          await expect.poll(() => driver.homeTourVisible(), { timeout: 15_000 }).toBe(true);
          shown = true;
        } catch {
          // Another pass at the round.
        }
      }
      expect(shown).toBe(true);
      for (let step = 0; step < 8 && (await driver.homeTourVisible()); step += 1) {
        await driver.homeTourAdvance();
      }
      await expect.poll(() => driver.homeTourVisible(), { timeout: 30_000 }).toBe(false);
      // The last step finishes into the create-project dialog; close it.
      await driver.page.keyboard.press("Escape");
      await expect.poll(() => serverTourCompleted(session), { timeout: 30_000 }).toBe(true);
    });
  }
);
