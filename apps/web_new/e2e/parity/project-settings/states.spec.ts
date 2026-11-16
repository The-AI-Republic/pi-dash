// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-392): project-settings workflow states tab.
// Rows PSET-028–035. Written against the driver interface only and green
// on apps/web through drivers/web.
//
// Observed behavior notes: the tab renders seven fixed group cards
// (backlog, unstarted, started, review, test, completed, cancelled); six
// start expanded while review starts collapsed. Creation and editing share
// one inline name/description/swatch form (name required, capped at 100
// characters, group-convention starter swatch); duplicate creates toast an
// already-exists notice and keep the form open. Rows drag with top/bottom
// placement; a one-state group offers no grip. Deletion confirms through a
// modal and refuses the default state, the last state of a group, and
// states holding work items (with move-items guidance). The default flag
// is exclusive with optimistic update. The member-editing switch is
// admin-only and gates the whole tab for members. Known old bugs (marked
// bug: in the titles): duplicate renames toast the generic failure and
// close the form (NEWFRONT-402); failed moves and promotions keep the
// optimistic result silently instead of rolling back with a notice
// (NEWFRONT-403); the loading skeleton never renders during fetch
// (NEWFRONT-405).
import { test, expect } from "../fixtures";
import {
  completeOnboarding,
  createServerState,
  parityProjectIdentifier,
  serverAddProjectMembers,
  serverCleanupProject,
  serverCleanupWorkspaceMember,
  serverCreateIssueFull,
  serverCreateProjectWithFlags,
  serverCreateState,
  serverDeleteState,
  serverListStates,
  serverPatchIssue,
  serverPatchProject,
  serverProvisionWorkspaceMember,
  serverWorkspaceMembers,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const GROUPS = ["backlog", "unstarted", "started", "review", "test", "completed", "cancelled"];

/** Fresh scenario-owned project; the server seeds its default states. */
async function setupProject(seed: { workspaceSlug: string }, tag: string, session: string): Promise<string> {
  return serverCreateProjectWithFlags(
    seed.workspaceSlug,
    `${tag} project`,
    parityProjectIdentifier("N392"),
    {},
    session
  );
}

/** The seeded default state of a fresh project, or a throw naming the gap. */
async function defaultStateName(workspaceSlug: string, projectId: string, session: string): Promise<string> {
  const states = await serverListStates(workspaceSlug, projectId, session);
  const current = states.find((s) => s.isDefault);
  if (!current) throw new Error("[parity] fresh project seeded no default state.");
  return current.name;
}

/**
 * Trim the cancelled group to a single state through the API (fresh
 * projects seed it without issues, so every row there is deletable
 * unless it is the default) and return the survivor's name.
 */
async function ensureSoloGroup(workspaceSlug: string, projectId: string, session: string): Promise<string> {
  const states = await serverListStates(workspaceSlug, projectId, session);
  const group = states.filter((s) => s.group === "cancelled" && !s.isDefault);
  for (const state of group.slice(1)) await serverDeleteState(workspaceSlug, projectId, state.id, session);
  const remaining = (await serverListStates(workspaceSlug, projectId, session)).filter((s) => s.group === "cancelled");
  if (remaining.length !== 1 || !remaining[0]) throw new Error("[parity] could not isolate a single-state group.");
  return remaining[0].name;
}

test(
  specTitle(["PSET-028"], "states tab renders seven fixed groups; readers get no editing affordances"),
  { tag: specTags(["PSET-028"]) },
  async ({ driver, seed }) => {
    const tag = `NF392 st28 ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await setupProject(seed, tag, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsStatesOpen(seed.workspaceSlug, projectId);

      await test.step("all seven group cards render with their states", async () => {
        expect(await driver.settingsStatesGroupNames()).toEqual(GROUPS);
        expect(await driver.settingsStatesAccessDenied()).toBe(false);
        // Review starts collapsed; every other group starts expanded.
        expect(await driver.settingsStatesGroupExpanded("review")).toBe(false);
        await driver.settingsStatesToggleGroup("review");
        for (const group of GROUPS) {
          expect(await driver.settingsStatesGroupExpanded(group)).toBe(true);
          expect((await driver.settingsStatesStateNames(group)).length).toBeGreaterThan(0);
        }
        const serverGroups = new Set(
          (await serverListStates(seed.workspaceSlug, projectId, session)).map((s) => s.group)
        );
        for (const group of GROUPS) expect(serverGroups.has(group)).toBe(true);
      });

      await test.step("admins get the add button in every group", async () => {
        for (const group of GROUPS) expect(await driver.settingsStatesAddEnabled(group)).toBe(true);
      });

      await test.step("members read the same list with editing removed once the flag is off", async () => {
        const provisioned = await serverProvisionWorkspaceMember(seed.workspaceSlug, "nf392s28", session);
        await serverAddProjectMembers(
          seed.workspaceSlug,
          projectId,
          [{ memberId: provisioned.userId, role: 15 }],
          session
        );
        await completeOnboarding(provisioned.email, provisioned.password);
        try {
          await serverPatchProject(seed.workspaceSlug, projectId, session, { members_can_edit_states: false });
          // Sign-in is a no-op while a session lives, so sign the admin
          // out first — otherwise this step still runs as the admin.
          await driver.signOutViaCommandPalette();
          await driver.rulesEnsureSignedIn(provisioned.email, provisioned.password, seed.workspaceSlug);
          await driver.settingsStatesOpen(seed.workspaceSlug, projectId);
          expect(await driver.settingsStatesAccessDenied()).toBe(false);
          expect(await driver.settingsStatesGroupNames()).toEqual(GROUPS);
          // The flag defaults to true until the project details land, so
          // wait for the read-only state to converge before asserting it.
          await expect.poll(() => driver.settingsStatesAddEnabled("backlog"), { timeout: 15_000 }).toBe(false);
          for (const group of GROUPS) expect(await driver.settingsStatesAddEnabled(group)).toBe(false);
          expect(await driver.settingsStatesMemberToggleVisible()).toBe(false);
        } finally {
          const memberships = await serverWorkspaceMembers(seed.workspaceSlug, session);
          const row = memberships.find((m) => m.userId === provisioned.userId);
          if (row) await serverCleanupWorkspaceMember(seed.workspaceSlug, row.membershipId, session);
        }
      });

      await test.step("outsiders see the restriction view instead of the list", async () => {
        if (!seed.guestEmail || !seed.guestPassword) throw new Error("[parity] seed has no guest identity.");
        await driver.signOutViaCommandPalette();
        await driver.rulesEnsureSignedIn(seed.guestEmail, seed.guestPassword, seed.workspaceSlug);
        await driver.settingsStatesOpen(seed.workspaceSlug, projectId);
        expect(await driver.settingsStatesAccessDenied()).toBe(true);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PSET-029"], "create state in group validates, caps the name, refuses duplicates, never triage"),
  { tag: specTags(["PSET-029"]) },
  async ({ driver, seed }) => {
    const tag = `NF392 st29 ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await setupProject(seed, tag, session);
    const name = `${tag} alpha`;
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsStatesOpen(seed.workspaceSlug, projectId);

      await test.step("the form opens docked in the group with a pre-tinted swatch", async () => {
        await driver.settingsStatesOpenCreate("backlog");
        expect(await driver.settingsStatesFormVisible()).toBe(true);
        expect(await driver.settingsStatesDotColor()).toMatch(/^rgb\(/);
      });

      await test.step("an empty name keeps the form open and saves nothing", async () => {
        await driver.settingsStatesAttemptSubmit();
        expect(await driver.settingsStatesFormVisible()).toBe(true);
        expect(
          (await serverListStates(seed.workspaceSlug, projectId, session)).some(
            (s) => s.group === "backlog" && s.name === ""
          )
        ).toBe(false);
      });

      await test.step("the name caps at 100 characters", async () => {
        await driver.settingsStatesFillName("x".repeat(101));
        expect((await driver.settingsStatesNameValue()).length).toBe(100);
        await driver.settingsStatesCancelForm();
      });

      await test.step("a valid create persists name, description and swatch", async () => {
        await driver.settingsStatesOpenCreate("backlog");
        await driver.settingsStatesOpenColorPicker();
        // The new palette is a fixed spread without the oracle's purple, so
        // the new run picks a member and matches its computed color.
        await driver.settingsStatesPickColor(driver.target === "web_new" ? "#8B5CF6" : "#9900EF");
        await expect
          .poll(() => driver.settingsStatesDotColor(), { timeout: 10_000 })
          .toBe(driver.target === "web_new" ? "rgb(139, 92, 246)" : "rgb(153, 0, 239)");
        await driver.settingsStatesFillName(name);
        await driver.settingsStatesFillDescription(`${tag} describes the flow`);
        await driver.settingsStatesSubmitCreate();
        await expect.poll(() => driver.settingsStatesStateNames("backlog"), { timeout: 15_000 }).toContain(name);
        const created = (await serverListStates(seed.workspaceSlug, projectId, session)).find((s) => s.name === name);
        expect(created?.group).toBe("backlog");
        await expect
          .poll(
            async () => {
              const toast = await driver.rulesLastToast();
              return toast ? `${toast.title} ${toast.message}` : "";
            },
            { timeout: 15_000 }
          )
          .toContain("State created successfully");
      });

      await test.step("a duplicate name toasts already-exists and keeps the form open", async () => {
        await driver.settingsStatesOpenCreate("backlog");
        await driver.settingsStatesFillName(name);
        // The toast reader reports the oldest visible toast, so let the
        // success toast from the valid create expire before firing the
        // error toast this step asserts.
        await expect.poll(() => driver.rulesLastToast(), { timeout: 30_000 }).toBeNull();
        await driver.settingsStatesAttemptSubmit();
        if (driver.target === "web_new") {
          // Intended (NEWFRONT-606): web_new answers a duplicate create
          // with its own notice copy instead of the oracle's server
          // message; the inventory pins the refusal, not the wording.
          await expect
            .poll(
              async () => {
                const toast = await driver.rulesLastToast();
                return toast ? `${toast.title} ${toast.message}` : "";
              },
              { timeout: 15_000 }
            )
            .toContain("Another state exists with the same name");
        } else {
          await expect
            .poll(
              async () => {
                const toast = await driver.rulesLastToast();
                return toast ? `${toast.title} ${toast.message}` : "";
              },
              { timeout: 15_000 }
            )
            .toContain("already exists");
        }
        expect(await driver.settingsStatesFormVisible()).toBe(true);
        await driver.settingsStatesCancelForm();
      });

      await test.step("no triage group exists to create into", async () => {
        expect(await driver.settingsStatesGroupNames()).not.toContain("triage");
        // The server enforces the same rule for direct writes.
        let refused = false;
        try {
          await createServerState(seed.workspaceSlug, projectId, session, {
            name: `${tag} smuggled`,
            group: "triage",
            color: "#FF6900",
          });
        } catch {
          refused = true;
        }
        expect(refused).toBe(true);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(
    ["PSET-030"],
    "bug: NEWFRONT-402 duplicate rename toasts generic and closes the form; rename, recolor and cancel hold"
  ),
  { tag: specTags(["PSET-030"]) },
  async ({ driver, seed }) => {
    const tag = `NF392 st30 ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await setupProject(seed, tag, session);
    const original = `${tag} beta`;
    const renamed = `${tag} gamma`;
    await createServerState(seed.workspaceSlug, projectId, session, {
      name: original,
      group: "unstarted",
      color: "#FF6900",
      description: `${tag} original blurb`,
    });
    const clash = `${tag} clash`;
    await serverCreateState(seed.workspaceSlug, projectId, clash, "unstarted", session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsStatesOpen(seed.workspaceSlug, projectId);

      await test.step("the edit form opens prefilled with name, description and swatch", async () => {
        await driver.settingsStatesOpenEdit(original);
        await expect.poll(() => driver.settingsStatesNameValue(), { timeout: 10_000 }).toBe(original);
        expect(await driver.settingsStatesDescriptionValue()).toBe(`${tag} original blurb`);
        await expect.poll(() => driver.settingsStatesDotColor(), { timeout: 10_000 }).toBe("rgb(255, 105, 0)");
      });

      await test.step("saving updates the row in place and on the server", async () => {
        await driver.settingsStatesFillName(renamed);
        await driver.settingsStatesFillDescription(`${tag} rewritten blurb`);
        await driver.settingsStatesOpenColorPicker();
        // The new app's palette is a fixed spread without the oracle's
        // twitter blue, so it recolors with a member it offers (NEWFRONT-606).
        await driver.settingsStatesPickColor(driver.target === "web_new" ? "#3B82F6" : "#0693E3");
        await driver.settingsStatesSubmitUpdate();
        await expect.poll(() => driver.settingsStatesStateNames("unstarted"), { timeout: 15_000 }).toContain(renamed);
        expect(await driver.settingsStatesStateNames("unstarted")).not.toContain(original);
        const updated = (await serverListStates(seed.workspaceSlug, projectId, session)).find(
          (s) => s.name === renamed
        );
        expect(updated?.group).toBe("unstarted");
      });

      await test.step("cancel restores the row untouched", async () => {
        await driver.settingsStatesOpenEdit(renamed);
        await driver.settingsStatesFillName(`${tag} abandoned`);
        await driver.settingsStatesCancelForm();
        await expect.poll(() => driver.settingsStatesStateNames("unstarted"), { timeout: 15_000 }).toContain(renamed);
        expect(
          (await serverListStates(seed.workspaceSlug, projectId, session)).some((s) => s.name === `${tag} abandoned`)
        ).toBe(false);
      });

      // bug: NEWFRONT-402 — the duplicate rename toasts the generic
      // failure message (the patch error carries no status for the
      // already-exists branch) and the optimistic rename unmounts the
      // form (rows key by name), where the intended behavior keeps the
      // prefilled form open with the specific message.
      await test.step("a duplicate rename toasts generic and closes the form, persisting nothing", async () => {
        await driver.settingsStatesOpenEdit(renamed);
        await driver.settingsStatesFillName(clash);
        // The toast reader reports the oldest visible toast, so let the
        // success toast from the save expire before firing the error
        // toast this step asserts.
        await expect.poll(() => driver.rulesLastToast(), { timeout: 30_000 }).toBeNull();
        await driver.settingsStatesAttemptSubmit();
        if (driver.target === "web_new") {
          // Intended (NEWFRONT-402, NEWFRONT-606): the prefilled form
          // stays open on the clash with the specific message.
          await expect
            .poll(
              async () => {
                const toast = await driver.rulesLastToast();
                return toast ? `${toast.title} ${toast.message}` : "";
              },
              { timeout: 15_000 }
            )
            .toContain("Another state exists with the same name");
          await expect.poll(() => driver.settingsStatesFormVisible(), { timeout: 15_000 }).toBe(true);
          expect(await driver.settingsStatesNameValue()).toBe(clash);
          // The open form replaces the row, so close it before the shared
          // rows assertion below.
          await driver.settingsStatesCancelForm();
          await expect.poll(() => driver.settingsStatesFormVisible(), { timeout: 15_000 }).toBe(false);
        } else {
          await expect
            .poll(
              async () => {
                const toast = await driver.rulesLastToast();
                return toast ? `${toast.title} ${toast.message}` : "";
              },
              { timeout: 15_000 }
            )
            .toContain("State could not be updated");
          await expect.poll(() => driver.settingsStatesFormVisible(), { timeout: 15_000 }).toBe(false);
        }
        // The server refused: exactly one clash remains and the rename
        // target is untouched.
        const rows = await serverListStates(seed.workspaceSlug, projectId, session);
        expect(rows.filter((s) => s.name === clash)).toHaveLength(1);
        expect(rows.some((s) => s.name === renamed)).toBe(true);
        await expect.poll(() => driver.settingsStatesStateNames("unstarted"), { timeout: 15_000 }).toContain(renamed);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(
    ["PSET-031"],
    "bug: NEWFRONT-403 failed move keeps the dropped position silently; reorder and cross-group moves hold"
  ),
  { tag: specTags(["PSET-031"]) },
  async ({ driver, seed }) => {
    const tag = `NF392 st31 ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await setupProject(seed, tag, session);
    const first = `${tag} one`;
    const second = `${tag} two`;
    const third = `${tag} three`;
    await serverCreateState(seed.workspaceSlug, projectId, first, "backlog", session);
    await serverCreateState(seed.workspaceSlug, projectId, second, "backlog", session);
    await serverCreateState(seed.workspaceSlug, projectId, third, "backlog", session);
    const target = `${tag} landing`;
    await serverCreateState(seed.workspaceSlug, projectId, target, "unstarted", session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsStatesOpen(seed.workspaceSlug, projectId);

      await test.step("the grip shows on hover for a multi-state group", async () => {
        expect(await driver.settingsStatesGripVisible(first)).toBe(true);
      });

      await test.step("dragging below reorders within the group and persists", async () => {
        const before = (await driver.settingsStatesStateNames("backlog")).filter((n) => n.startsWith(tag));
        expect(before).toEqual([first, second, third]);
        await driver.settingsStatesDragBelow(first, second);
        await expect
          .poll(async () => (await driver.settingsStatesStateNames("backlog")).filter((n) => n.startsWith(tag)), {
            timeout: 20_000,
          })
          .toEqual([second, first, third]);
      });

      await test.step("dragging across groups moves the state and persists", async () => {
        await driver.settingsStatesDragBelow(first, target);
        await expect.poll(() => driver.settingsStatesStateNames("unstarted"), { timeout: 20_000 }).toContain(first);
        expect(await driver.settingsStatesStateNames("backlog")).not.toContain(first);
        const moved = (await serverListStates(seed.workspaceSlug, projectId, session)).find((s) => s.name === first);
        expect(moved?.group).toBe("unstarted");
      });

      await test.step("a single-state group offers no grip", async () => {
        const solo = await ensureSoloGroup(seed.workspaceSlug, projectId, session);
        await driver.settingsStatesOpen(seed.workspaceSlug, projectId);
        expect(await driver.settingsStatesGripVisible(solo)).toBe(false);
      });

      // bug: NEWFRONT-403 — the failed move keeps the optimistic
      // placement (the store restores a live reference, a no-op) and
      // stays silent, where the intended behavior rolls back with a
      // notice.
      await test.step("a failed move keeps the dropped position with no notice; the server is untouched", async () => {
        await driver.page.route("**/api/workspaces/*/projects/*/states/*/", async (route) => {
          if (route.request().method() === "PATCH") {
            await route.fulfill({ status: 500, body: "{}" });
            return;
          }
          await route.continue();
        });
        try {
          // first sits in unstarted after the cross-group move; drop it
          // back over backlog with the save refused.
          await driver.settingsStatesDragBelow(first, second);
          if (driver.target === "web_new") {
            // Intended (NEWFRONT-403, NEWFRONT-606): the drop rolls back
            // to unstarted with a failure notice.
            await expect.poll(() => driver.settingsStatesStateNames("unstarted"), { timeout: 15_000 }).toContain(first);
            expect(await driver.settingsStatesStateNames("backlog")).not.toContain(first);
            await expect
              .poll(
                async () => {
                  const toast = await driver.rulesLastToast();
                  return toast ? `${toast.title} ${toast.message}` : "";
                },
                { timeout: 15_000 }
              )
              .toContain("State could not be updated");
          } else {
            await expect.poll(() => driver.settingsStatesStateNames("backlog"), { timeout: 15_000 }).toContain(first);
            expect(await driver.settingsStatesStateNames("unstarted")).not.toContain(first);
            // Nothing toasts: give a notice time to appear, then confirm none did.
            await driver.page.waitForTimeout(2000);
            expect(await driver.rulesLastToast()).toBeNull();
          }
          const stuck = (await serverListStates(seed.workspaceSlug, projectId, session)).find((s) => s.name === first);
          expect(stuck?.group).toBe("unstarted");
        } finally {
          await driver.page.unroute("**/api/workspaces/*/projects/*/states/*/");
        }
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PSET-032"], "delete confirms; default, last-in-group and non-empty states survive"),
  { tag: specTags(["PSET-032"]) },
  async ({ driver, seed }) => {
    const tag = `NF392 st32 ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await setupProject(seed, tag, session);
    const doomed = `${tag} doomed`;
    await serverCreateState(seed.workspaceSlug, projectId, doomed, "backlog", session);
    const busy = `${tag} busy`;
    const busyId = await serverCreateState(seed.workspaceSlug, projectId, busy, "started", session);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    await serverPatchIssue(seed.workspaceSlug, projectId, issue.id, { state_id: busyId }, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsStatesOpen(seed.workspaceSlug, projectId);

      await test.step("the confirmation names the state and warns it is permanent", async () => {
        await driver.settingsStatesDelete(doomed);
        const modal = await driver.settingsStatesDeleteModalText();
        expect(modal).toContain(doomed);
        expect(modal).toMatch(/permanently|undo/i);
      });

      await test.step("cancel leaves the state untouched", async () => {
        await driver.settingsStatesDeleteCancel();
        expect(await driver.settingsStatesDeleteModalText()).toBe("");
        await expect.poll(() => driver.settingsStatesStateNames("backlog"), { timeout: 15_000 }).toContain(doomed);
      });

      await test.step("confirm deletes it from the list and the server", async () => {
        await driver.settingsStatesDelete(doomed);
        await driver.settingsStatesDeleteConfirm();
        await expect.poll(() => driver.settingsStatesStateNames("backlog"), { timeout: 15_000 }).not.toContain(doomed);
        expect((await serverListStates(seed.workspaceSlug, projectId, session)).some((s) => s.name === doomed)).toBe(
          false
        );
      });

      await test.step("the default state cannot be deleted", async () => {
        const current = await defaultStateName(seed.workspaceSlug, projectId, session);
        expect(await driver.settingsStatesIsDefault(current)).toBe(true);
        expect(await driver.settingsStatesDeleteDisabled(current)).toBe(true);
      });

      await test.step("a state holding work items is rejected with move-items guidance", async () => {
        await driver.settingsStatesDelete(busy);
        await driver.settingsStatesAttemptDeleteConfirm();
        if (driver.target === "web_new") {
          // Intended (NEWFRONT-606): web_new guides with its own notice
          // copy ("...work items. Move them..."); the inventory pins the
          // guidance, not the oracle's exact wording.
          await expect
            .poll(
              async () => {
                const toast = await driver.rulesLastToast();
                return toast ? `${toast.title} ${toast.message}` : "";
              },
              { timeout: 15_000 }
            )
            .toContain("Move them");
        } else {
          await expect
            .poll(
              async () => {
                const toast = await driver.rulesLastToast();
                return toast ? `${toast.title} ${toast.message}` : "";
              },
              { timeout: 15_000 }
            )
            .toContain("move them");
        }
        expect((await serverListStates(seed.workspaceSlug, projectId, session)).some((s) => s.name === busy)).toBe(
          true
        );
      });

      await test.step("the last state of a group cannot be deleted", async () => {
        const solo = await ensureSoloGroup(seed.workspaceSlug, projectId, session);
        await driver.settingsStatesOpen(seed.workspaceSlug, projectId);
        expect(await driver.settingsStatesDeleteDisabled(solo)).toBe(true);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(
    ["PSET-033"],
    "bug: NEWFRONT-403 failed promotion keeps the stale marker silently; exclusive promotion holds"
  ),
  { tag: specTags(["PSET-033"]) },
  async ({ driver, seed }) => {
    const tag = `NF392 st33 ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await setupProject(seed, tag, session);
    const challenger = `${tag} challenger`;
    await serverCreateState(seed.workspaceSlug, projectId, challenger, "backlog", session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsStatesOpen(seed.workspaceSlug, projectId);
      const previous = await defaultStateName(seed.workspaceSlug, projectId, session);

      await test.step("the current default carries a static marker, not an action", async () => {
        expect(await driver.settingsStatesIsDefault(previous)).toBe(true);
        expect(await driver.settingsStatesIsDefault(challenger)).toBe(false);
      });

      await test.step("promoting flips the exclusive flag on screen and server", async () => {
        await driver.settingsStatesMarkDefault(challenger);
        await expect.poll(() => driver.settingsStatesIsDefault(challenger), { timeout: 15_000 }).toBe(true);
        await expect.poll(() => driver.settingsStatesIsDefault(previous), { timeout: 15_000 }).toBe(false);
        // The marker flips optimistically; the server write lands after.
        await expect
          .poll(() => defaultStateName(seed.workspaceSlug, projectId, session), { timeout: 20_000 })
          .toBe(challenger);
      });

      // bug: NEWFRONT-403 — the failed promotion keeps the optimistic
      // marker (the store restores a live reference, a no-op) and stays
      // silent, where the intended behavior rolls back with a notice.
      await test.step("a failed promotion keeps the stale marker with no notice; the server is untouched", async () => {
        await driver.page.route("**/api/workspaces/*/projects/*/states/*/mark-default/", async (route) => {
          await route.fulfill({ status: 500, body: "{}" });
        });
        try {
          await driver.settingsStatesMarkDefault(previous);
          if (driver.target === "web_new") {
            // Intended (NEWFRONT-403, NEWFRONT-606): the marker rolls back
            // to the challenger with a failure notice.
            await expect.poll(() => driver.settingsStatesIsDefault(previous), { timeout: 15_000 }).toBe(false);
            await expect.poll(() => driver.settingsStatesIsDefault(challenger), { timeout: 15_000 }).toBe(true);
            await expect
              .poll(
                async () => {
                  const toast = await driver.rulesLastToast();
                  return toast ? `${toast.title} ${toast.message}` : "";
                },
                { timeout: 15_000 }
              )
              .toContain("State could not be made the default");
          } else {
            await expect.poll(() => driver.settingsStatesIsDefault(previous), { timeout: 15_000 }).toBe(true);
            expect(await driver.settingsStatesIsDefault(challenger)).toBe(false);
            // Nothing toasts: give a notice time to appear, then confirm none did.
            await driver.page.waitForTimeout(2000);
            expect(await driver.rulesLastToast()).toBeNull();
          }
          expect(await defaultStateName(seed.workspaceSlug, projectId, session)).toBe(challenger);
        } finally {
          await driver.page.unroute("**/api/workspaces/*/projects/*/states/*/mark-default/");
        }
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PSET-034"], "the admin toggle gates member state editing and the server mirrors it"),
  { tag: specTags(["PSET-034"]) },
  async ({ driver, seed }) => {
    const tag = `NF392 st34 ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await setupProject(seed, tag, session);
    // Fresh projects seed one state per group (gripless by design), so add
    // backlog states up front for the member grip assertions below.
    const gripExtra = `${tag} grip extra`;
    await serverCreateState(seed.workspaceSlug, projectId, `${tag} grip primer`, "backlog", session);
    await serverCreateState(seed.workspaceSlug, projectId, gripExtra, "backlog", session);
    const provisioned = await serverProvisionWorkspaceMember(seed.workspaceSlug, "nf392s34", session);
    await serverAddProjectMembers(seed.workspaceSlug, projectId, [{ memberId: provisioned.userId, role: 15 }], session);
    await completeOnboarding(provisioned.email, provisioned.password);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsStatesOpen(seed.workspaceSlug, projectId);

      await test.step("admins see the toggle and it starts on", async () => {
        await expect.poll(() => driver.settingsStatesMemberToggleVisible(), { timeout: 15_000 }).toBe(true);
        expect(await driver.settingsStatesMemberToggleOn()).toBe(true);
      });

      await test.step("with the flag on, members get full editing", async () => {
        // Sign-in is a no-op while a session lives, so sign out before
        // every identity switch in this test.
        await driver.signOutViaCommandPalette();
        await driver.rulesEnsureSignedIn(provisioned.email, provisioned.password, seed.workspaceSlug);
        await driver.settingsStatesOpen(seed.workspaceSlug, projectId);
        await expect.poll(() => driver.settingsStatesAddEnabled("backlog"), { timeout: 15_000 }).toBe(true);
        await expect.poll(() => driver.settingsStatesStateNames("backlog"), { timeout: 15_000 }).toContain(gripExtra);
        expect(await driver.settingsStatesGripVisible(gripExtra)).toBe(true);
      });

      await test.step("flipping it off reads back off immediately", async () => {
        await driver.signOutViaCommandPalette();
        await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
        await driver.settingsStatesOpen(seed.workspaceSlug, projectId);
        await driver.settingsStatesSetMemberToggle(false);
        await expect.poll(() => driver.settingsStatesMemberToggleOn(), { timeout: 15_000 }).toBe(false);
      });

      await test.step("with the flag off, members read without editing and writes are refused", async () => {
        await driver.signOutViaCommandPalette();
        await driver.rulesEnsureSignedIn(provisioned.email, provisioned.password, seed.workspaceSlug);
        await driver.settingsStatesOpen(seed.workspaceSlug, projectId);
        expect(await driver.settingsStatesAccessDenied()).toBe(false);
        await expect.poll(() => driver.settingsStatesAddEnabled("backlog"), { timeout: 15_000 }).toBe(false);
        // The server mirrors the gate: a member write is refused.
        let refused = false;
        try {
          await serverCreateState(seed.workspaceSlug, projectId, `${tag} smuggled`, "backlog", provisioned.session);
        } catch {
          refused = true;
        }
        expect(refused).toBe(true);
      });

      await test.step("flipping it back on restores member editing", async () => {
        await driver.signOutViaCommandPalette();
        await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
        await driver.settingsStatesOpen(seed.workspaceSlug, projectId);
        await driver.settingsStatesSetMemberToggle(true);
        await expect.poll(() => driver.settingsStatesMemberToggleOn(), { timeout: 15_000 }).toBe(true);
        // The member UI with the flag on is proven above; here the
        // restored server gate is enough (and spares another switch).
        const restored = `${tag} restored`;
        await serverCreateState(seed.workspaceSlug, projectId, restored, "backlog", provisioned.session);
        expect((await serverListStates(seed.workspaceSlug, projectId, session)).some((s) => s.name === restored)).toBe(
          true
        );
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
  specTitle(["PSET-035"], "bug: NEWFRONT-405 loading never shows the skeleton; empty notes and rollback hold"),
  { tag: specTags(["PSET-035"]) },
  async ({ driver, seed }) => {
    const tag = `NF392 st35 ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await setupProject(seed, tag, session);
    const states = await serverListStates(seed.workspaceSlug, projectId, session);
    // Empty one group through the API so the empty note has a clean stage.
    const cancelled = states.filter((s) => s.group === "cancelled" && !s.isDefault);
    for (const state of cancelled) await serverDeleteState(seed.workspaceSlug, projectId, state.id, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      // bug: NEWFRONT-405 — the loader is gated on the project id
      // rather than the fetch, so its skeleton never renders; the
      // cards render empty and populate when the answers land.
      await test.step("loading renders empty cards with no skeleton, then populates", async () => {
        // Settle on the page first, then cold-boot straight into the
        // held load via reload: a plain entry would reuse the landing
        // page's already-fetched states and skip loading entirely.
        await driver.openPath(`/${seed.workspaceSlug}/settings/projects/${projectId}/states`);
        await expect.poll(() => driver.settingsStatesGroupNames(), { timeout: 30_000 }).toEqual(GROUPS);
        await driver.settingsStatesDelayLoad(20000);
        await driver.page.reload();
        if (driver.target === "web_new") {
          // Intended (NEWFRONT-405, NEWFRONT-606): the skeleton precedes
          // the content while the held answers are in flight.
          await expect.poll(() => driver.settingsStatesSkeletonVisible(), { timeout: 30_000 }).toBe(true);
          await expect.poll(() => driver.settingsStatesStateNames("backlog"), { timeout: 60_000 }).toContain("Backlog");
          expect(await driver.settingsStatesSkeletonVisible()).toBe(false);
          return;
        }
        await expect.poll(() => driver.settingsStatesGroupNames(), { timeout: 30_000 }).toEqual(GROUPS);
        // Cards render before the held answers land: empty, note
        // showing, and not a skeleton in sight.
        expect(await driver.settingsStatesStateNames("backlog")).toEqual([]);
        expect(await driver.settingsStatesEmptyNote("backlog")).toMatch(/No states available/i);
        expect(await driver.settingsStatesSkeletonVisible()).toBe(false);
        await driver.page.waitForTimeout(3000);
        expect(await driver.settingsStatesSkeletonVisible()).toBe(false);
        await expect.poll(() => driver.settingsStatesStateNames("backlog"), { timeout: 60_000 }).toContain("Backlog");
        expect(await driver.settingsStatesSkeletonVisible()).toBe(false);
      });

      await test.step("an emptied group shows its note plus a create prompt for editors", async () => {
        expect(await driver.settingsStatesStateNames("cancelled")).toEqual([]);
        expect(await driver.settingsStatesEmptyNote("cancelled")).toMatch(/No states available/i);
        expect(await driver.settingsStatesAddEnabled("cancelled")).toBe(true);
      });

      // bug: NEWFRONT-402 — the failed rename toasts the generic
      // failure (the patch error carries no status) and the optimistic
      // rename unmounts the form (rows key by name); the data itself
      // rolls back correctly.
      await test.step("a failed rename rolls back with a notice and no error panel", async () => {
        const names = await driver.settingsStatesStateNames("backlog");
        const victim = names[0] ?? "";
        await driver.page.route("**/api/workspaces/*/projects/*/states/*/", async (route) => {
          if (route.request().method() === "PATCH") {
            await route.fulfill({ status: 500, body: "{}" });
            return;
          }
          await route.continue();
        });
        try {
          await driver.settingsStatesOpenEdit(victim);
          await driver.settingsStatesFillName(`${tag} ghost`);
          await driver.settingsStatesAttemptSubmit();
          await expect
            .poll(
              async () => {
                const toast = await driver.rulesLastToast();
                return toast ? `${toast.title} ${toast.message}` : "";
              },
              { timeout: 15_000 }
            )
            .toContain("State could not be updated");
          if (driver.target === "web_new") {
            // Intended (NEWFRONT-402, NEWFRONT-606): the form keeps the
            // failed draft open (edits intact) while the row underneath
            // rolls back; only a success closes it.
            await expect.poll(() => driver.settingsStatesFormVisible(), { timeout: 15_000 }).toBe(true);
            expect(await driver.settingsStatesNameValue()).toBe(`${tag} ghost`);
            // The open form replaces the row, so close it before the shared
            // rows assertion below.
            await driver.settingsStatesCancelForm();
            await expect.poll(() => driver.settingsStatesFormVisible(), { timeout: 15_000 }).toBe(false);
          } else {
            // The failed submit closes the form; the row restores underneath.
            await expect.poll(() => driver.settingsStatesFormVisible(), { timeout: 15_000 }).toBe(false);
          }
          await expect.poll(() => driver.settingsStatesStateNames("backlog"), { timeout: 15_000 }).toContain(victim);
          expect(
            (await serverListStates(seed.workspaceSlug, projectId, session)).some((s) => s.name === `${tag} ghost`)
          ).toBe(false);
        } finally {
          await driver.page.unroute("**/api/workspaces/*/projects/*/states/*/");
        }
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
