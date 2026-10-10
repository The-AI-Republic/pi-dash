// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-32): duplicating a draft with its payload
// carried over, and deleting through the confirmation dialog with the
// creator-or-project-admin gate. Rows: DRAFT-010, DRAFT-011. Green on
// apps/web first.
import { test, expect } from "../fixtures";
import {
  ROLE,
  serverCreateDraft,
  serverCreateState,
  serverDeleteDraftStatus,
  serverDraftNames,
  serverDraftsPage,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { draftsHarness, draftsOpenAs, draftsSeat } from "./support";

test(
  specTitle(["DRAFT-010"], "duplicating carries the payload into a marked copy"),
  { tag: specTags(["DRAFT-010"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d10");
    const { owner, workspaceSlug, projectId, projectIdentifier } = harness;
    const name = `D10 Source ${harness.tag}`;
    const copy = `${name} (copy)`;
    const state2 = await serverCreateState(workspaceSlug, projectId, "Doing", "started", owner.cookie);
    const description = `<p>Carried body ${harness.tag}</p>`;
    const source = await serverCreateDraft(workspaceSlug, owner.cookie, {
      name,
      project_id: projectId,
      description_html: description,
      state_id: state2,
      priority: "high",
    });

    await test.step("duplicate opens the carried payload", async () => {
      await draftsOpenAs(driver, harness);
      await driver.draftRowMenuClick(name, "make_a_copy");
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(true);
      expect(await driver.createTitleValue()).toBe(copy);
    });

    await test.step("saving stores the marked copy next to the original", async () => {
      await driver.submitCreateModal();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
      await expect.poll(() => driver.draftRowNames(), { timeout: 60_000 }).toContain(copy);
      expect(await driver.draftRowProjectMarker(copy)).toBe(projectIdentifier);
      expect(await driver.draftPriorityText(copy)).toBe("High");
    });

    await test.step("the server holds the copy with the source's values", async () => {
      await expect.poll(() => serverDraftNames(workspaceSlug, owner.cookie), { timeout: 60_000 }).toContain(copy);
      const page = await serverDraftsPage(workspaceSlug, owner.cookie, "");
      const stored = page.drafts.find((draft) => draft.name === copy);
      expect(stored).toBeDefined();
      expect(stored?.projectId).toBe(source.projectId);
      expect(stored?.descriptionHtml).toContain(`Carried body ${harness.tag}`);
      expect(stored?.stateId).toBe(source.stateId);
      expect(stored?.priority).toBe(source.priority);
    });
  }
);

test(
  specTitle(["DRAFT-011"], "confirming delete removes the draft with a success notice"),
  { tag: specTags(["DRAFT-011"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d11");
    const { owner, workspaceSlug, projectId } = harness;
    const name = `D11 Gone ${harness.tag}`;
    await serverCreateDraft(workspaceSlug, owner.cookie, { name, project_id: projectId });

    await test.step("confirming removes the row and reports success", async () => {
      await draftsOpenAs(driver, harness);
      await driver.draftRowMenuClick(name, "delete");
      await driver.confirmDraftDelete();
      await expect.poll(() => driver.toastText(), { timeout: 30_000 }).toContain("Draft deleted");
      await expect.poll(() => driver.draftRowNames(), { timeout: 60_000 }).not.toContain(name);
      await expect.poll(() => serverDraftNames(workspaceSlug, owner.cookie), { timeout: 60_000 }).not.toContain(name);
    });
  }
);

test(
  specTitle(["DRAFT-011"], "deleting another user's draft is refused unless a project admin"),
  { tag: specTags(["DRAFT-011"]) },
  async ({ driver }) => {
    // The drafts list is owner-scoped, so a non-creator never reaches the
    // UI confirm path; the gate below is the server's, which the dialog
    // mirrors for project admins.
    const harness = await draftsHarness("parity-d11b");
    const { owner, workspaceSlug, projectId } = harness;
    const name = `D11 Foreign ${harness.tag}`;
    const draft = await serverCreateDraft(workspaceSlug, owner.cookie, { name, project_id: projectId });
    const member = await draftsSeat(harness, ROLE.MEMBER, "parity-d11-member", ROLE.MEMBER);
    const guest = await draftsSeat(harness, ROLE.GUEST, "parity-d11-guest");
    const projectAdmin = await draftsSeat(harness, ROLE.MEMBER, "parity-d11-admin", ROLE.ADMIN);
    const workspaceAdmin = await draftsSeat(harness, ROLE.ADMIN, "parity-d11-wsadmin");

    await test.step("a member, a guest and a project-only admin are refused and the draft survives", async () => {
      // The server's delete gate is creator-or-WORKSPACE-admin even though
      // the dialog's client gate also offers the attempt to project
      // admins; the owner-scoped list keeps the UI attempt unreachable,
      // so the server's refusal is the observable behavior.
      expect((await serverDeleteDraftStatus(workspaceSlug, draft.id, member.cookie)).status).toBe(403);
      expect((await serverDeleteDraftStatus(workspaceSlug, draft.id, guest.cookie)).status).toBe(403);
      expect((await serverDeleteDraftStatus(workspaceSlug, draft.id, projectAdmin.cookie)).status).toBe(403);
      expect(await serverDraftNames(workspaceSlug, owner.cookie)).toContain(name);
    });

    await test.step("a workspace admin removes it", async () => {
      expect((await serverDeleteDraftStatus(workspaceSlug, draft.id, workspaceAdmin.cookie)).status).toBe(204);
      await expect.poll(() => serverDraftNames(workspaceSlug, owner.cookie), { timeout: 60_000 }).not.toContain(name);
    });

    await test.step("the owner's screen no longer shows it", async () => {
      await draftsOpenAs(driver, harness);
      expect(await driver.draftRowNames()).not.toContain(name);
    });
  }
);
