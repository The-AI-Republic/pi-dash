// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-122): label grouping and reorder —
// dragging a label onto another nests it as a child (two-level tree),
// dropping onto a child lands it as a sibling (no third level),
// self-drops and parent-onto-child drops are refused, groups cannot be
// nested (they reorder instead), edge drops reorder with fractional
// sort orders, and "Remove from group" clears the parent. Row: ISS-229
// (group / ungroup labels and reorder).
//
// Observed behavior notes (inventory row carries them at update time):
// drops PATCH {parent, sort_order} with a fractional midpoint order
// (half the next order at the head, midpoint inside, +10000 at the
// tail, server-assigned into an empty group); dropping a group onto a
// label reorders the group above it instead of nesting; children never
// receive drops (a drop onto a child lands above it as a sibling).
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverCleanupProject,
  serverCreateLabel,
  serverCreateProjectWithFlags,
  serverLabels,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["ISS-229"], "label drag nests children, caps at two levels, and refuses bad drops"),
  { tag: specTags(["ISS-229"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 lblg ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      {},
      session
    );
    const a = await serverCreateLabel(seed.workspaceSlug, projectId, `${tag} A`, "#FF6900", session);
    const b = await serverCreateLabel(seed.workspaceSlug, projectId, `${tag} B`, "#FCB900", session);
    const c = await serverCreateLabel(seed.workspaceSlug, projectId, `${tag} C`, "#00D084", session);
    const parentOf = async (id: string) =>
      (await serverLabels(seed.workspaceSlug, projectId, session)).find((l) => l.id === id)?.parent;
    const sortOf = async (id: string) =>
      (await serverLabels(seed.workspaceSlug, projectId, session)).find((l) => l.id === id)?.sortOrder;
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsLabelsOpen(seed.workspaceSlug, projectId);
      await expect.poll(() => driver.settingsLabelsNames(), { timeout: 15_000 }).toEqual([a.name, b.name, c.name]);

      await test.step("dropping B onto A nests B as its child", async () => {
        await driver.settingsLabelsDragOnto(b.name, a.name);
        await expect.poll(() => parentOf(b.id), { timeout: 15_000 }).toBe(a.id);
        expect(await driver.settingsLabelsIsGroup(a.name)).toBe(true);
        await driver.settingsLabelsOpenRowMenu(b.name);
        expect(await driver.settingsLabelsMenuItems()).toEqual(["Remove from group", "Edit label"]);
        await driver.pickerPressEscape();
      });

      await test.step("dropping C onto the child B lands C as a sibling, not a grandchild", async () => {
        await driver.settingsLabelsDragOnto(c.name, b.name);
        await expect.poll(() => parentOf(c.id), { timeout: 15_000 }).toBe(a.id);
        expect(await driver.settingsLabelsIsGroup(b.name)).toBe(false);
      });

      await test.step("self-drops and parent-onto-child drops change nothing", async () => {
        // Absence of change has no event to poll, so settle briefly and
        // then compare; the ungroup steps below prove the page is alive.
        const settle = () => new Promise((resolve) => setTimeout(resolve, 2_000));
        const sortBefore = await sortOf(a.id);
        await driver.settingsLabelsDragOnto(a.name, a.name);
        await settle();
        expect(await parentOf(a.id)).toBe(null);
        expect(await sortOf(a.id)).toBe(sortBefore);
        await driver.settingsLabelsDragOnto(a.name, b.name);
        await settle();
        expect(await parentOf(a.id)).toBe(null);
        expect(await sortOf(a.id)).toBe(sortBefore);
        expect(await parentOf(b.id)).toBe(a.id);
      });

      await test.step("remove-from-group clears the parent and dissolves the group", async () => {
        await driver.settingsLabelsOpenRowMenu(b.name);
        await driver.settingsLabelsMenuPick("Remove from group");
        await expect.poll(() => parentOf(b.id), { timeout: 15_000 }).toBe(null);
        expect(await driver.settingsLabelsIsGroup(a.name)).toBe(true);
        await driver.settingsLabelsOpenRowMenu(c.name);
        await driver.settingsLabelsMenuPick("Remove from group");
        await expect.poll(() => parentOf(c.id), { timeout: 15_000 }).toBe(null);
        expect(await driver.settingsLabelsIsGroup(a.name)).toBe(false);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ISS-229"], "label drag reorders with fractional orders and never nests a group"),
  { tag: specTags(["ISS-229"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 lblr ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      {},
      session
    );
    const x = await serverCreateLabel(seed.workspaceSlug, projectId, `${tag} X`, "#8ED1FC", session);
    const y = await serverCreateLabel(seed.workspaceSlug, projectId, `${tag} Y`, "#0693E3", session);
    const z = await serverCreateLabel(seed.workspaceSlug, projectId, `${tag} Z`, "#9900EF", session);
    const sortOf = async (id: string) =>
      (await serverLabels(seed.workspaceSlug, projectId, session)).find((l) => l.id === id)?.sortOrder;
    const sortNum = async (id: string): Promise<number> => {
      const value = await sortOf(id);
      if (value === undefined) throw new Error(`[parity] label ${id} vanished from the server list.`);
      return value;
    };
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsLabelsOpen(seed.workspaceSlug, projectId);
      await expect.poll(() => driver.settingsLabelsNames(), { timeout: 15_000 }).toContain(x.name);

      await test.step("dropping the last label above the first reorders it to the head", async () => {
        const names = await driver.settingsLabelsNames();
        const first = names[0]!;
        const last = names[names.length - 1]!;
        const firstId = [x, y, z].find((l) => l.name === first)!.id;
        const lastId = [x, y, z].find((l) => l.name === last)!.id;
        const firstSort = await sortNum(firstId);
        await driver.settingsLabelsDragAbove(last, first);
        await expect.poll(() => sortNum(lastId), { timeout: 15_000 }).toBeLessThan(firstSort);
        const rest = names.filter((n) => n !== first && n !== last);
        await expect.poll(() => driver.settingsLabelsNames(), { timeout: 15_000 }).toEqual([last, first, ...rest]);
      });

      await test.step("dropping a group onto a label reorders it instead of nesting", async () => {
        const names = await driver.settingsLabelsNames();
        // Nest the middle label under the head, then drop the head group
        // onto the tail: the group must reorder above the tail with a
        // null parent, never become its child.
        const head = names[0]!;
        const middle = names[1]!;
        const tail = names[2]!;
        const headId = [x, y, z].find((l) => l.name === head)!.id;
        const tailId = [x, y, z].find((l) => l.name === tail)!.id;
        await driver.settingsLabelsDragOnto(middle, head);
        await expect
          .poll(
            async () =>
              (await serverLabels(seed.workspaceSlug, projectId, session)).find((l) => l.name === middle)?.parent,
            { timeout: 15_000 }
          )
          .toBe(headId);
        await driver.settingsLabelsDragOnto(head, tail);
        const tailSort = await sortNum(tailId);
        await expect.poll(() => sortNum(headId), { timeout: 15_000 }).toBeLessThan(tailSort);
        const headRow = (await serverLabels(seed.workspaceSlug, projectId, session)).find((l) => l.id === headId);
        expect(headRow?.parent).toBe(null);
        expect(await driver.settingsLabelsIsGroup(head)).toBe(true);
        await expect.poll(() => driver.settingsLabelsNames(), { timeout: 15_000 }).toEqual([head, middle, tail]);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
