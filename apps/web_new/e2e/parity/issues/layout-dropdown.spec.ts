// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios (NEWFRONT-122): layout dropdown — the view-create
// form's layout picker is a search-disabled dropdown over the five
// layouts with a checkmark on the selected one; picking a layout sticks
// and persists on the saved view. Rows: ISS-219 (layout dropdown).
//
// Observed behavior notes (inventory row carries them at update time):
// all five layouts always render — the disabledLayouts prop has no
// callers passing it (dead, like the ISS-212 current-cycle filter).
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverCleanupProject,
  serverCleanupView,
  serverCreateProjectWithFlags,
  serverView,
  serverViews,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["ISS-219"], "layout picker lists five layouts, marks the selected one and persists it"),
  { tag: specTags(["ISS-219"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 layout ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      { issueViewsView: true },
      session
    );
    const viewName = `${tag} view`;
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.viewsOpenList(seed.workspaceSlug, projectId);
      await driver.viewsOpenCreate();

      await test.step("the trigger defaults to the list layout", async () => {
        await expect.poll(() => driver.viewsLayoutValue(), { timeout: 15_000 }).toContain("List");
      });

      await test.step("the picker lists every layout with no search and marks the selection", async () => {
        await driver.viewsLayoutOpenPicker();
        await expect
          .poll(() => driver.viewsLayoutOptionTexts(), { timeout: 10_000 })
          .toEqual(["List", "Board", "Calendar", "Table", "Timeline"]);
        expect(await driver.viewsLayoutHasSearch()).toBe(false);
        expect(await driver.viewsLayoutSelectedMarked("List")).toBe(true);
        expect(await driver.viewsLayoutSelectedMarked("Board")).toBe(false);
      });

      await test.step("picking a layout sticks and persists on the saved view", async () => {
        await driver.viewsLayoutPick("Board");
        await expect.poll(() => driver.viewsLayoutValue(), { timeout: 10_000 }).toContain("Board");
        await driver.viewsLayoutOpenPicker();
        expect(await driver.viewsLayoutSelectedMarked("Board")).toBe(true);
        await driver.pickerPressEscape();
        await driver.viewsFillName(viewName);
        await driver.viewsSubmit();
        let viewId = "";
        await expect
          .poll(
            async () => {
              const found = (await serverViews(seed.workspaceSlug, projectId, session)).find(
                (v) => v.name === viewName
              );
              if (found !== undefined) viewId = found.id;
              return found?.id;
            },
            { timeout: 15_000 }
          )
          .not.toBe(undefined);
        const created = await serverView(seed.workspaceSlug, projectId, viewId, session);
        expect(created.layout).toBe("kanban");
      });
    } finally {
      const doomed = (await serverViews(seed.workspaceSlug, projectId, session)).find((v) => v.name === viewName);
      if (doomed !== undefined && doomed.id !== "")
        await serverCleanupView(seed.workspaceSlug, projectId, doomed.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
