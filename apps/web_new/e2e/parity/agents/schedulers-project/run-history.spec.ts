// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-185): an install's run history re-fetches on
// its own about every quarter minute without reloading the page; with no
// runs there is no pager; opening the detail of a removed install explains
// the install may have been uninstalled and offers a way back to the list.
// Row: AGT-014.
// Note: paging across pages needs 31+ runs on one install, and the seeded
// stack never fires (no beat), so the pager half is a Gap — the no-pager
// state with zero runs is asserted instead.
import { test, expect } from "../../fixtures";
import {
  ensureProject,
  ensureScheduler,
  parityProjectIdentifier,
  serverBindings,
  serverCreateBinding,
  serverDeleteBinding,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";

const ROWS = ["AGT-014"];

test(
  specTitle(ROWS, "run history re-fetches itself; removed installs explain with a way back"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-ag14"));
    const { owner, ownerSession, workspaceSlug, tag } = harness;
    const handle = `agt14-main-${workspaceSlug}`;

    const project = await test.step("owner prepares a project with one install", async () => {
      const created = await ensureProject(
        workspaceSlug,
        ownerSession,
        `AGT14 Project ${tag}`,
        parityProjectIdentifier("AG14")
      );
      const definition = await ensureScheduler(workspaceSlug, ownerSession, {
        slug: handle,
        name: "AGT14 Main Definition",
        prompt: "Audit this project nightly.",
        is_enabled: true,
      });
      const rows = await serverBindings(workspaceSlug, created.id, ownerSession);
      if (!rows.some((row) => row.scheduler === definition.id)) {
        await serverCreateBinding(workspaceSlug, created.id, ownerSession, {
          scheduler: definition.id,
          project: created.id,
          dtstart: new Date(Date.now() + 24 * 3600_000).toISOString(),
          rrule: "FREQ=DAILY",
        });
      }
      return created;
    });
    const bindingId = async (): Promise<string> => {
      const found = (await serverBindings(workspaceSlug, project.id, ownerSession)).find(
        (row) => row.scheduler_slug === handle
      );
      if (found === undefined) throw new Error("[parity] expected the AGT14 install.");
      return found.id;
    };

    await test.step("owner signs in and opens the install detail", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.schedulerOpenProjectBinding(workspaceSlug, project.id, await bindingId());
      expect(await driver.schedulerBindingRunsCount()).toBe("0 runs");
    });

    await test.step("the history re-fetches about every quarter minute without a reload", async () => {
      // Reveal the prompt first: its revealed state surviving the refetch
      // proves the page never reloaded.
      await driver.schedulerBindingPromptToggle();
      expect(await driver.schedulerBindingPromptState()).toEqual({ toggleVisible: true, revealed: true });
      await driver.schedulerBindingWaitRunsRefetch();
      const first = Date.now();
      await driver.schedulerBindingWaitRunsRefetch();
      const gapMs = Date.now() - first;
      expect(gapMs).toBeGreaterThan(8_000);
      expect(gapMs).toBeLessThan(28_000);
      expect(await driver.schedulerBindingPromptState()).toEqual({ toggleVisible: true, revealed: true });
      const header = await driver.schedulerBindingHeader();
      expect(header.name).toBe("AGT14 Main Definition");
      expect(await driver.schedulerBindingRuns()).toEqual([]);
      expect(await driver.schedulerBindingRunsCount()).toBe("0 runs");
      expect(await driver.schedulerBindingRunsPager()).toBeNull();
    });

    await test.step("a removed install explains itself with a way back", async () => {
      const bid = await bindingId();
      await serverDeleteBinding(workspaceSlug, project.id, bid, ownerSession);
      await driver.schedulerOpenProjectBinding(workspaceSlug, project.id, bid);
      expect(await driver.schedulerBindingRemovedVisible()).toBe(true);
      await driver.schedulerBindingBackToList();
      expect(await driver.schedulerProjectEmptyVisible()).toBe(true);
      expect(await driver.schedulerProjectRows()).toEqual([]);
    });
  }
);
