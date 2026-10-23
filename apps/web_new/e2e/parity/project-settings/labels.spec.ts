// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-392): project-settings labels tab. Rows
// PSET-036–042. Written against the driver interface only and green on
// apps/web through drivers/web.
//
// Observed behavior notes: labels form a two-level tree (parents render
// as expandable group cards starting open; plain labels show a color
// dot); creation uses an inline form with a random starter swatch, a
// required title capped at 255 characters, and an already-exists notice
// for duplicates. Dragging nests (highlight ring), reorders (insertion
// line), moves between parents, and reorders whole groups, while
// self/own-child/child-row drops no-op; non-admins cannot drag at all.
// Children detach through their row menu; deletion confirms and strips
// the label from carrying work items with no undo. The skeleton hides
// while the creation form is open; an empty project shows an illustrated
// panel whose action opens creation.
import { test, expect } from "../fixtures";
import {
  completeOnboarding,
  parityProjectIdentifier,
  serverAddProjectMembers,
  serverCleanupProject,
  serverCleanupWorkspaceMember,
  serverCreateIssueFull,
  serverCreateLabel,
  serverCreateProjectWithFlags,
  serverIssue,
  serverLabelOrNull,
  serverLabels,
  serverPatchLabel,
  serverProvisionWorkspaceMember,
  serverSetIssueLabels,
  serverWorkspaceMembers,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

/** Fresh scenario-owned project (no labels seeded) with unique names. */
async function setupProject(seed: { workspaceSlug: string }, tag: string, session: string): Promise<string> {
  return serverCreateProjectWithFlags(
    seed.workspaceSlug,
    `${tag} project`,
    parityProjectIdentifier("N392"),
    {},
    session
  );
}

test(
  specTitle(["PSET-036"], "labels tab shows the two-level hierarchy; members read without editing"),
  { tag: specTags(["PSET-036"]) },
  async ({ driver, seed }) => {
    const tag = `NF392 lb36 ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await setupProject(seed, tag, session);
    const parent = await serverCreateLabel(seed.workspaceSlug, projectId, `${tag} parent`, "#FF6900", session);
    const childName = `${tag} child`;
    const child = await serverCreateLabel(seed.workspaceSlug, projectId, childName, "#0693E3", session);
    await serverPatchLabel(seed.workspaceSlug, projectId, child.id, { parent: parent.id }, session);
    const plainName = `${tag} plain`;
    await serverCreateLabel(seed.workspaceSlug, projectId, plainName, "#00D084", session);
    const provisioned = await serverProvisionWorkspaceMember(seed.workspaceSlug, "nf392l36", session);
    await serverAddProjectMembers(seed.workspaceSlug, projectId, [{ memberId: provisioned.userId, role: 15 }], session);
    await completeOnboarding(provisioned.email, provisioned.password);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsLabelsOpen(seed.workspaceSlug, projectId);

      await test.step("parents render as groups with their children nested", async () => {
        expect(await driver.settingsLabelsIsGroup(`${tag} parent`)).toBe(true);
        expect(await driver.settingsLabelsIsGroup(plainName)).toBe(false);
        expect(await driver.settingsLabelsChildrenOf(`${tag} parent`)).toEqual([childName]);
      });

      await test.step("group cards collapse and expand", async () => {
        expect(await driver.settingsLabelsGroupCollapsed(`${tag} parent`)).toBe(false);
        await driver.settingsLabelsToggleGroup(`${tag} parent`);
        expect(await driver.settingsLabelsGroupCollapsed(`${tag} parent`)).toBe(true);
        expect(await driver.settingsLabelsChildrenOf(`${tag} parent`)).toEqual([]);
        await driver.settingsLabelsToggleGroup(`${tag} parent`);
        expect(await driver.settingsLabelsChildrenOf(`${tag} parent`)).toEqual([childName]);
      });

      await test.step("members see the same list with no editing affordances", async () => {
        // Sign-in is a no-op while a session lives, so sign the admin
        // out first — otherwise this step still runs as the admin.
        await driver.signOutViaCommandPalette();
        await driver.rulesEnsureSignedIn(provisioned.email, provisioned.password, seed.workspaceSlug);
        await driver.settingsLabelsOpen(seed.workspaceSlug, projectId);
        expect(await driver.settingsLabelsAccessDenied()).toBe(false);
        expect(await driver.settingsLabelsAddVisible()).toBe(false);
        expect(await driver.settingsLabelsGripVisible(plainName)).toBe(false);
        expect(await driver.settingsLabelsChildrenOf(`${tag} parent`)).toEqual([childName]);
      });
    } finally {
      const memberships = await serverWorkspaceMembers(seed.workspaceSlug, session);
      const row = memberships.find((m) => m.userId === provisioned.userId);
      if (row) await serverCleanupWorkspaceMember(seed.workspaceSlug, row.membershipId, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PSET-037"], "create label validates, seeds a starter swatch, refuses duplicates"),
  { tag: specTags(["PSET-037"]) },
  async ({ driver, seed }) => {
    const tag = `NF392 lb37 ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await setupProject(seed, tag, session);
    const name = `${tag} alpha`;
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsLabelsOpen(seed.workspaceSlug, projectId);

      await test.step("a fresh form starts with a rendered starter swatch", async () => {
        await driver.settingsLabelsOpenCreate();
        expect(await driver.settingsLabelsDotColor()).toMatch(/^rgb\(/);
      });

      await test.step("empty and over-long titles fail inline", async () => {
        await driver.settingsLabelsAttemptSubmit();
        await expect.poll(() => driver.settingsLabelsFormError(), { timeout: 10_000 }).toBe("Label title is required");
        await driver.settingsLabelsFillName("x".repeat(256));
        await driver.settingsLabelsAttemptSubmit();
        await expect
          .poll(() => driver.settingsLabelsFormError(), { timeout: 10_000 })
          .toBe("Label name should not exceed 255 characters");
        expect(await driver.settingsLabelsFormVisible()).toBe(true);
      });

      await test.step("a valid create persists with the picked swatch", async () => {
        const before = await driver.settingsLabelsDotColor();
        const pick = before === "rgb(0, 208, 132)" ? "#9900EF" : "#00D084";
        await driver.settingsLabelsOpenColorPicker();
        await driver.settingsLabelsPickColor(pick);
        await driver.settingsLabelsFillName(name);
        await driver.settingsLabelsSubmitCreate();
        await expect.poll(() => driver.settingsLabelsNames(), { timeout: 15_000 }).toContain(name);
        const created = (await serverLabels(seed.workspaceSlug, projectId, session)).find((l) => l.name === name);
        expect(created?.color.toLowerCase()).toBe(pick.toLowerCase());
      });

      await test.step("a duplicate name surfaces the already-exists notice", async () => {
        await driver.settingsLabelsOpenCreate();
        await driver.settingsLabelsFillName(name);
        await driver.settingsLabelsAttemptSubmit();
        await expect
          .poll(
            async () => {
              const toast = await driver.rulesLastToast();
              return toast ? `${toast.title} ${toast.message}` : "";
            },
            { timeout: 15_000 }
          )
          .toContain("Label already exists");
        expect(await driver.settingsLabelsFormVisible()).toBe(true);
        await driver.settingsLabelsCancelForm();
      });

      await test.step("a failed create surfaces a notice with the form kept open", async () => {
        await driver.settingsLabelsOpenCreate();
        await driver.page.route("**/issue-labels/*", async (route) => {
          if (route.request().method() === "POST") {
            await route.fulfill({ status: 500, body: "{}" });
            return;
          }
          await route.continue();
        });
        try {
          await driver.settingsLabelsFillName(`${tag} ghost`);
          // The toast reader reports the oldest visible toast, so let
          // the duplicate notice expire before firing this failure.
          await expect.poll(() => driver.rulesLastToast(), { timeout: 30_000 }).toBeNull();
          await driver.settingsLabelsAttemptSubmit();
          await expect
            .poll(
              async () => {
                const toast = await driver.rulesLastToast();
                return toast ? `${toast.title} ${toast.message}` : "";
              },
              { timeout: 15_000 }
            )
            .not.toBe("");
          expect(await driver.settingsLabelsFormVisible()).toBe(true);
          expect(
            (await serverLabels(seed.workspaceSlug, projectId, session)).some((l) => l.name === `${tag} ghost`)
          ).toBe(false);
        } finally {
          await driver.page.unroute("**/issue-labels/*");
        }
        await driver.settingsLabelsCancelForm();
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PSET-038"], "rename and recolor through the prefilled inline form"),
  { tag: specTags(["PSET-038"]) },
  async ({ driver, seed }) => {
    const tag = `NF392 lb38 ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await setupProject(seed, tag, session);
    const original = `${tag} beta`;
    const label = await serverCreateLabel(seed.workspaceSlug, projectId, original, "#FF6900", session);
    const clash = `${tag} clash`;
    await serverCreateLabel(seed.workspaceSlug, projectId, clash, "#00D084", session);
    const renamed = `${tag} gamma`;
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsLabelsOpen(seed.workspaceSlug, projectId);

      await test.step("the edit form opens prefilled with name and swatch", async () => {
        await driver.settingsLabelsOpenRowMenu(original);
        expect(await driver.settingsLabelsMenuItems()).toContain("Edit label");
        await driver.settingsLabelsMenuPick("Edit label");
        await expect.poll(() => driver.settingsLabelsNameValue(), { timeout: 10_000 }).toBe(original);
        await expect.poll(() => driver.settingsLabelsDotColor(), { timeout: 10_000 }).toBe("rgb(255, 105, 0)");
      });

      await test.step("rename and recolor persist to the server", async () => {
        await driver.settingsLabelsFillName(renamed);
        await driver.settingsLabelsOpenColorPicker();
        await driver.settingsLabelsPickColor("#9900EF");
        await driver.settingsLabelsSubmitUpdate();
        await expect.poll(() => driver.settingsLabelsNames(), { timeout: 15_000 }).toContain(renamed);
        const updated = await serverLabelOrNull(seed.workspaceSlug, projectId, label.id, session);
        expect(updated?.name).toBe(renamed);
        expect(updated?.color.toLowerCase()).toBe("#9900ef");
      });

      await test.step("cancel restores the row untouched", async () => {
        await driver.settingsLabelsOpenRowMenu(renamed);
        await driver.settingsLabelsMenuPick("Edit label");
        await driver.settingsLabelsFillName(`${tag} abandoned`);
        await driver.settingsLabelsCancelForm();
        await expect.poll(() => driver.settingsLabelsNames(), { timeout: 15_000 }).toContain(renamed);
        expect((await serverLabelOrNull(seed.workspaceSlug, projectId, label.id, session))?.name).toBe(renamed);
      });

      await test.step("a duplicate rename is refused with the form kept open", async () => {
        await driver.settingsLabelsOpenRowMenu(renamed);
        await driver.settingsLabelsMenuPick("Edit label");
        await driver.settingsLabelsFillName(clash);
        // The form reads React state on submit, so let the fill flush
        // before submitting (a quick submit sends the stale prefilled
        // name and no-ops with a 200).
        await expect.poll(() => driver.settingsLabelsNameValue(), { timeout: 10_000 }).toBe(clash);
        await driver.page.waitForTimeout(3000);
        await driver.settingsLabelsAttemptSubmit();
        await expect
          .poll(
            async () => {
              const toast = await driver.rulesLastToast();
              return toast ? `${toast.title} ${toast.message}` : "";
            },
            { timeout: 15_000 }
          )
          .toContain("same name already exists");
        expect(await driver.settingsLabelsFormVisible()).toBe(true);
        await driver.settingsLabelsCancelForm();
        expect((await serverLabelOrNull(seed.workspaceSlug, projectId, label.id, session))?.name).toBe(renamed);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PSET-039"], "bug: NEWFRONT-408 failed drops roll back silently; reorder, nesting and rejections hold"),
  { tag: specTags(["PSET-039"]) },
  async ({ driver, seed }) => {
    const tag = `NF392 lb39 ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await setupProject(seed, tag, session);
    const anchor = `${tag} anchor`;
    const drifter = `${tag} drifter`;
    const anchorRow = await serverCreateLabel(seed.workspaceSlug, projectId, anchor, "#FF6900", session);
    const drifterRow = await serverCreateLabel(seed.workspaceSlug, projectId, drifter, "#0693E3", session);
    const other = `${tag} other`;
    const otherRow = await serverCreateLabel(seed.workspaceSlug, projectId, other, "#00D084", session);
    const lone = `${tag} lone`;
    await serverCreateLabel(seed.workspaceSlug, projectId, lone, "#EB144C", session);
    const provisioned = await serverProvisionWorkspaceMember(seed.workspaceSlug, "nf392l39", session);
    await serverAddProjectMembers(seed.workspaceSlug, projectId, [{ memberId: provisioned.userId, role: 15 }], session);
    await completeOnboarding(provisioned.email, provisioned.password);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsLabelsOpen(seed.workspaceSlug, projectId);

      await test.step("reordering shows the insertion line and persists the order", async () => {
        const feedback = await driver.settingsLabelsDragFeedback(drifter, anchor, "above");
        expect(feedback).toBe("line");
        await expect
          .poll(async () => (await driver.settingsLabelsNames()).filter((n) => n.startsWith(tag)), { timeout: 20_000 })
          .toEqual([drifter, anchor, other, lone]);
        const order = (await serverLabels(seed.workspaceSlug, projectId, session))
          .filter((l) => l.name.startsWith(tag))
          .sort((a, b) => a.sortOrder - b.sortOrder)
          .map((l) => l.name);
        expect(order[0]).toBe(drifter);
      });

      await test.step("nesting shows the highlight ring and persists the parent", async () => {
        const feedback = await driver.settingsLabelsDragFeedback(drifter, anchor, "nest");
        expect(feedback).toBe("ring");
        await expect.poll(() => driver.settingsLabelsChildrenOf(anchor), { timeout: 20_000 }).toEqual([drifter]);
        expect((await serverLabelOrNull(seed.workspaceSlug, projectId, drifterRow.id, session))?.parent).toBe(
          anchorRow.id
        );
      });

      await test.step("a child moves between parents and persists", async () => {
        const feedback = await driver.settingsLabelsDragFeedback(drifter, other, "nest");
        expect(feedback).toBe("ring");
        await expect.poll(() => driver.settingsLabelsChildrenOf(other), { timeout: 20_000 }).toEqual([drifter]);
        expect(await driver.settingsLabelsChildrenOf(anchor)).toEqual([]);
        expect((await serverLabelOrNull(seed.workspaceSlug, projectId, drifterRow.id, session))?.parent).toBe(
          otherRow.id
        );
      });

      await test.step("whole groups reorder at top level with their children", async () => {
        await driver.settingsLabelsDragAbove(other, anchor);
        await expect
          .poll(async () => (await driver.settingsLabelsNames()).filter((n) => n.startsWith(tag)), { timeout: 20_000 })
          .toEqual([other, drifter, anchor, lone]);
        expect((await serverLabelOrNull(seed.workspaceSlug, projectId, otherRow.id, session))?.parent).toBe(null);
      });

      await test.step("self, group-onto-child and parent-onto-child drops no-op", async () => {
        await driver.settingsLabelsDragOnto(anchor, anchor);
        await driver.page.waitForTimeout(2000);
        expect((await serverLabelOrNull(seed.workspaceSlug, projectId, anchorRow.id, session))?.parent).toBe(null);
        // `other` is a group and `drifter` its own child: nesting refuses it.
        await driver.settingsLabelsDragOnto(other, drifter);
        await driver.page.waitForTimeout(2000);
        expect((await serverLabelOrNull(seed.workspaceSlug, projectId, otherRow.id, session))?.parent).toBe(null);
        expect(await driver.settingsLabelsChildrenOf(other)).toEqual([drifter]);
      });

      await test.step("drops onto a child row place as siblings under its parent", async () => {
        // Grandchild creation is blocked (make-child is refused for
        // child targets), but the drop itself is allowed: it lands as
        // a sibling under the child's parent.
        const loneRow = (await serverLabels(seed.workspaceSlug, projectId, session)).find((l) => l.name === lone);
        if (!loneRow) throw new Error("[parity] lone label missing before child-row drop.");
        await driver.settingsLabelsDragAbove(lone, drifter);
        await expect.poll(() => driver.settingsLabelsChildrenOf(other), { timeout: 15_000 }).toEqual([lone, drifter]);
        expect((await serverLabelOrNull(seed.workspaceSlug, projectId, loneRow.id, session))?.parent).toBe(otherRow.id);
      });

      // bug: NEWFRONT-408 — the drop handler never awaits the save, so
      // a failed drop rolls back with no notice, where the row wants a
      // notice with the rollback.
      await test.step("a failed drop rolls back with no notice; the server is untouched", async () => {
        const uiBefore = await driver.settingsLabelsNames();
        const orderBefore = new Map(
          (await serverLabels(seed.workspaceSlug, projectId, session))
            .filter((l) => l.name.startsWith(tag))
            .map((l) => [l.id, l.sortOrder] as const)
        );
        await driver.page.route("**/api/workspaces/*/projects/*/issue-labels/*/", async (route) => {
          if (route.request().method() === "PATCH") {
            // Let the optimistic placement land before refusing it, so
            // the test observes both the drop and the rollback.
            await new Promise((resolve) => setTimeout(resolve, 1500));
            await route.fulfill({ status: 500, body: "{}" });
            return;
          }
          await route.continue();
        });
        try {
          await driver.settingsLabelsDragAbove(lone, anchor);
          await expect
            .poll(async () => JSON.stringify(await driver.settingsLabelsNames()), { timeout: 15_000 })
            .not.toBe(JSON.stringify(uiBefore));
          await expect.poll(() => driver.settingsLabelsNames(), { timeout: 15_000 }).toEqual(uiBefore);
          const orderAfter = new Map(
            (await serverLabels(seed.workspaceSlug, projectId, session))
              .filter((l) => l.name.startsWith(tag))
              .map((l) => [l.id, l.sortOrder] as const)
          );
          expect(orderAfter).toEqual(orderBefore);
          if (driver.target === "web_new") {
            // Intended (NEWFRONT-408, NEWFRONT-606): the rollback lands
            // with a failure notice.
            await expect
              .poll(
                async () => {
                  const toast = await driver.rulesLastToast();
                  return toast ? `${toast.title} ${toast.message}` : "";
                },
                { timeout: 15_000 }
              )
              .toContain("Error while updating the label");
          } else {
            // Nothing toasts: give a notice time to appear, then confirm none did.
            await driver.page.waitForTimeout(2000);
            expect(await driver.rulesLastToast()).toBeNull();
          }
        } finally {
          await driver.page.unroute("**/api/workspaces/*/projects/*/issue-labels/*/");
        }
      });

      await test.step("dragging is disabled for non-admins", async () => {
        await driver.signOutViaCommandPalette();
        await driver.rulesEnsureSignedIn(provisioned.email, provisioned.password, seed.workspaceSlug);
        await driver.settingsLabelsOpen(seed.workspaceSlug, projectId);
        expect(await driver.settingsLabelsGripVisible(lone)).toBe(false);
      });
    } finally {
      const memberships = await serverWorkspaceMembers(seed.workspaceSlug, session);
      const row = memberships.find((m) => m.userId === provisioned.userId);
      if (row) await serverCleanupWorkspaceMember(seed.workspaceSlug, row.membershipId, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PSET-040"], "detach a child label back to the top level from its row menu"),
  { tag: specTags(["PSET-040"]) },
  async ({ driver, seed }) => {
    const tag = `NF392 lb40 ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await setupProject(seed, tag, session);
    const parent = await serverCreateLabel(seed.workspaceSlug, projectId, `${tag} parent`, "#FF6900", session);
    const childName = `${tag} child`;
    const child = await serverCreateLabel(seed.workspaceSlug, projectId, childName, "#0693E3", session);
    await serverPatchLabel(seed.workspaceSlug, projectId, child.id, { parent: parent.id }, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsLabelsOpen(seed.workspaceSlug, projectId);
      await expect
        .poll(() => driver.settingsLabelsChildrenOf(`${tag} parent`), { timeout: 15_000 })
        .toEqual([childName]);

      await driver.settingsLabelsOpenRowMenu(childName);
      expect(await driver.settingsLabelsMenuItems()).toContain("Remove from group");
      await driver.settingsLabelsMenuPick("Remove from group");
      await expect.poll(() => driver.settingsLabelsChildrenOf(`${tag} parent`), { timeout: 15_000 }).toEqual([]);
      await expect.poll(() => driver.settingsLabelsNames(), { timeout: 15_000 }).toContain(childName);
      expect((await serverLabelOrNull(seed.workspaceSlug, projectId, child.id, session))?.parent).toBe(null);
      // The detached label keeps its name and color at top level.
      const detached = await serverLabelOrNull(seed.workspaceSlug, projectId, child.id, session);
      expect(detached?.name).toBe(childName);
      expect(detached?.color.toLowerCase()).toBe("#0693e3");

      await test.step("the detached label stays top-level on reload", async () => {
        await driver.settingsLabelsOpen(seed.workspaceSlug, projectId);
        expect(await driver.settingsLabelsChildrenOf(`${tag} parent`)).toEqual([]);
        await expect.poll(() => driver.settingsLabelsNames(), { timeout: 15_000 }).toContain(childName);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PSET-041"], "delete label confirms and strips it from carrying work items"),
  { tag: specTags(["PSET-041"]) },
  async ({ driver, seed }) => {
    const tag = `NF392 lb41 ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await setupProject(seed, tag, session);
    const doomedName = `${tag} doomed`;
    const doomed = await serverCreateLabel(seed.workspaceSlug, projectId, doomedName, "#EB144C", session);
    const keeperName = `${tag} keeper`;
    const keeper = await serverCreateLabel(seed.workspaceSlug, projectId, keeperName, "#0693E3", session);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    await serverSetIssueLabels(seed.workspaceSlug, projectId, issue.id, [doomed.id], session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsLabelsOpen(seed.workspaceSlug, projectId);

      await test.step("the confirmation names the label and warns about removal", async () => {
        await driver.settingsLabelsDeleteViaTrash(doomedName);
        const modal = await driver.settingsLabelsDeleteModalText();
        expect(modal).toContain(doomedName);
        expect(modal).toMatch(/work item|view/i);
      });

      await test.step("cancel leaves the label untouched", async () => {
        await driver.settingsLabelsDeleteCancel();
        expect(await driver.settingsLabelsDeleteModalText()).toBe("");
        await expect.poll(() => driver.settingsLabelsNames(), { timeout: 15_000 }).toContain(doomedName);
      });

      await test.step("a failed delete toasts and keeps the label", async () => {
        await driver.page.route("**/api/workspaces/*/projects/*/issue-labels/*/", async (route) => {
          if (route.request().method() === "DELETE") {
            await route.fulfill({ status: 500, body: "{}" });
            return;
          }
          await route.continue();
        });
        try {
          await driver.settingsLabelsDeleteViaTrash(keeperName);
          await driver.settingsLabelsDeleteAttemptConfirm();
          await expect
            .poll(
              async () => {
                const toast = await driver.rulesLastToast();
                return toast ? `${toast.title} ${toast.message}` : "";
              },
              { timeout: 15_000 }
            )
            .toContain("Label could not be deleted");
          expect(await driver.settingsLabelsDeleteModalText()).not.toBe("");
          expect(await serverLabelOrNull(seed.workspaceSlug, projectId, keeper.id, session)).not.toBe(null);
          await driver.settingsLabelsDeleteCancel();
        } finally {
          await driver.page.unroute("**/api/workspaces/*/projects/*/issue-labels/*/");
        }
      });

      await test.step("confirm removes it everywhere with no undo", async () => {
        await driver.settingsLabelsDeleteViaTrash(doomedName);
        await driver.settingsLabelsDeleteConfirm();
        await expect.poll(() => driver.settingsLabelsNames(), { timeout: 15_000 }).not.toContain(doomedName);
        expect(await serverLabelOrNull(seed.workspaceSlug, projectId, doomed.id, session)).toBe(null);
        expect((await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).label_ids).toEqual([]);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PSET-042"], "labels skeleton hides with the form open; empty panel offers creation"),
  { tag: specTags(["PSET-042"]) },
  async ({ driver, seed }) => {
    const tag = `NF392 lb42 ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await setupProject(seed, tag, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("a skeleton precedes the list on a cold load", async () => {
        await driver.settingsLabelsDelayLoad(8000);
        await driver.settingsLabelsOpen(seed.workspaceSlug, projectId);
        await expect.poll(() => driver.settingsLabelsSkeletonVisible(), { timeout: 20_000 }).toBe(true);
      });

      await test.step("opening creation suppresses the skeleton even mid-load", async () => {
        await driver.settingsLabelsOpenCreate();
        expect(await driver.settingsLabelsFormVisible()).toBe(true);
        expect(await driver.settingsLabelsSkeletonVisible()).toBe(false);
        await driver.settingsLabelsCancelForm();
        await expect.poll(() => driver.settingsLabelsEmptyTitle(), { timeout: 60_000 }).toBe("No labels yet");
      });

      await test.step("the empty panel action opens creation", async () => {
        await driver.settingsLabelsEmptyAction();
        expect(await driver.settingsLabelsFormVisible()).toBe(true);
        await driver.settingsLabelsCancelForm();
      });

      await test.step("a failed create surfaces a notice with the form kept open", async () => {
        await driver.settingsLabelsOpenCreate();
        await driver.page.route("**/issue-labels/*", async (route) => {
          if (route.request().method() === "POST") {
            await route.fulfill({ status: 500, body: "{}" });
            return;
          }
          await route.continue();
        });
        try {
          await driver.settingsLabelsFillName(`${tag} ghost`);
          await driver.settingsLabelsAttemptSubmit();
          await expect
            .poll(
              async () => {
                const toast = await driver.rulesLastToast();
                return toast ? `${toast.title} ${toast.message}` : "";
              },
              { timeout: 15_000 }
            )
            .not.toBe("");
          expect(await driver.settingsLabelsFormVisible()).toBe(true);
          expect(await driver.settingsLabelsEmptyTitle()).toBe("");
        } finally {
          await driver.page.unroute("**/issue-labels/*");
        }
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
