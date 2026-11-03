// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-125): the overflow slide-over holds searchable
// extra projects with create, copy-link, reorder and an empty-search state.
// Observed on the running old app: once the project cap from the display
// preferences leaves joined projects out of the main sidebar, a More
// toggle opens a slide-over with a search box; typing filters by name or
// short code, keeping only matching projects; a hopeless query shows the
// empty state; clicking outside closes the panel. Row: SHELL-052.
// Cap discipline: the row limit lives on the seed user, so this scenario
// is the only one that ever lowers it, and it restores the uncapped
// baseline in the final step. Every guard below re-pins the cap because
// sibling runs share the seed user and can restore the default mid-step.
// (A dedicated member was tried: member self-join confers no project
// membership row on this stack, so the member's sidebar renders no rows;
// the invite endpoint that would grant it 500s per NEWFRONT-151.)
import { test, expect } from "../../fixtures";
import { deleteProject, ensureProject, patchUserProperties, ownerSession } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-052"];
// Deliberately distinct from the drag spec's second project: the two
// scenarios run in parallel workers and must never share, reorder or
// delete each other's project.
const SECOND_NAME = "Parity Sidebar Overflow";
const SECOND_CODE = "PAR_SO";
const THIRD_NAME = "Parity Sidebar Tertiary";
const THIRD_CODE = "PAR_ST";

test(
  specTitle(ROWS, "overflow slide-over holds searchable extra projects"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const session = await test.step("prepare server session", async () => ownerSession(seed));
    const second = await test.step("provision a second project", async () =>
      ensureProject(seed.workspaceSlug, session, SECOND_NAME, SECOND_CODE));
    const third = await test.step("provision a third project", async () =>
      ensureProject(seed.workspaceSlug, session, THIRD_NAME, THIRD_CODE));

    await test.step("sign in through the UI", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("capping the list reveals the overflow toggle", async () => {
      await patchUserProperties(seed.workspaceSlug, session, { navigation_project_limit: 1 });
      // A loaded page whose project fetch failed under load keeps its empty
      // list, so re-enter until the joined project shows instead of polling
      // a dead render; the final poll keeps the exact failure semantics.
      let seen = false;
      for (let attempt = 0; attempt < 3 && !seen; attempt += 1) {
        await driver.openWorkspaceHome(seed.workspaceSlug);
        await patchUserProperties(seed.workspaceSlug, session, { navigation_project_limit: 1 });
        const deadline = Date.now() + 20_000;
        do {
          if ((await driver.sidebarLinkTexts()).includes(SECOND_NAME)) {
            seen = true;
            break;
          }
          await driver.page.waitForTimeout(2000);
        } while (Date.now() < deadline);
      }
      // Sibling runs share the seed user and can join a newer project, which
      // would push this scenario's project into the panel; either placement
      // proves the cap overflowed while keeping the later steps honest.
      const secondReachable = async (): Promise<boolean> => {
        if ((await driver.sidebarLinkTexts()).includes(SECOND_NAME)) return true;
        if (await driver.isProjectsOverflowVisible()) {
          await driver.setProjectsOverflowOpen(true);
          return (await driver.overflowProjectNames()).some((name) => name.includes(SECOND_NAME));
        }
        return false;
      };
      await expect.poll(secondReachable, { timeout: 60_000 }).toBe(true);
      await expect.poll(() => driver.isProjectsOverflowVisible(), { timeout: 30_000 }).toBe(true);
    });

    await test.step("the panel lists extra projects with a create affordance", async () => {
      await patchUserProperties(seed.workspaceSlug, session, { navigation_project_limit: 1 });
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.isProjectsOverflowVisible(), { timeout: 30_000 }).toBe(true);
      await driver.setProjectsOverflowOpen(true);
      await expect.poll(() => driver.isProjectsOverflowOpen(), { timeout: 30_000 }).toBe(true);
      // The cap keeps one project in the sidebar; the overflowed project
      // surfaces in the panel. Panel rows carry extra chrome around the name,
      // so match by inclusion rather than exact membership.
      await expect
        .poll(async () => (await driver.overflowProjectNames()).some((name) => name.includes(seed.projectName)), {
          timeout: 30_000,
        })
        .toBe(true);
      await expect.poll(() => driver.isOverflowCreateVisible(), { timeout: 30_000 }).toBe(true);
    });

    await test.step("search filters by name, excluding non-matches", async () => {
      // Exclusion-shaped: a no-op search box would keep every row, so the
      // proof asserts non-matching panel entries disappear, not only that
      // the match stays. The baseline is read live because a sibling run
      // may hold its own project in the panel at the same time.
      const baseline = await driver.overflowProjectNames();
      expect(baseline.some((name) => name.includes(SECOND_NAME))).toBe(true);
      const excluded = baseline.filter((name) => !name.toLowerCase().includes("overflow"));
      expect(excluded.length).toBeGreaterThan(0);
      await driver.searchOverflowProjects("Overflow");
      const panelHas = (name: string) => async () =>
        (await driver.overflowProjectNames()).some((entry) => entry.includes(name));
      await expect.poll(panelHas(SECOND_NAME), { timeout: 30_000 }).toBe(true);
      for (const gone of excluded) {
        await expect.poll(panelHas(gone), { timeout: 30_000 }).toBe(false);
      }
    });

    await test.step("search filters by a code-only substring", async () => {
      // "_SO" appears in the PAR_SO short code but in no project name, so
      // only the code path can match it. Clear the previous query first so
      // the baseline below is the full panel, not its filtered remainder.
      await driver.searchOverflowProjects("");
      await expect
        .poll(async () => (await driver.overflowProjectNames()).some((name) => name.includes(seed.projectName)), {
          timeout: 30_000,
        })
        .toBe(true);
      const baseline = await driver.overflowProjectNames();
      const excluded = baseline.filter((name) => !name.includes(SECOND_NAME));
      expect(excluded.length).toBeGreaterThan(0);
      await driver.searchOverflowProjects("_SO");
      const panelHas = (name: string) => async () =>
        (await driver.overflowProjectNames()).some((entry) => entry.includes(name));
      await expect.poll(panelHas(SECOND_NAME), { timeout: 30_000 }).toBe(true);
      for (const gone of excluded) {
        await expect.poll(panelHas(gone), { timeout: 30_000 }).toBe(false);
      }
    });

    await test.step("a hopeless query shows the empty state", async () => {
      await driver.searchOverflowProjects("zzz-no-such-project");
      await expect.poll(() => driver.isOverflowEmptyStateVisible(), { timeout: 30_000 }).toBe(true);
    });

    await test.step("clicking outside closes the panel", async () => {
      await driver.clickMainContent();
      await expect.poll(() => driver.isProjectsOverflowOpen(), { timeout: 30_000 }).toBe(false);
    });

    await test.step("restore the seeded baseline", async () => {
      try {
        await deleteProject(seed.workspaceSlug, second.id, session);
        await deleteProject(seed.workspaceSlug, third.id, session);
      } finally {
        // The cap restore must run even when a delete fails: a leaked cap
        // hides every sibling run's sidebar rows.
        await patchUserProperties(seed.workspaceSlug, session, { navigation_project_limit: 10 });
      }
    });
  }
);
