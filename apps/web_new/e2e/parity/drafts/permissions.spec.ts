// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-32): the drafts permission matrix — guests
// create and remove their own drafts, members edit their own inline, and
// the server refuses cross-user and guest-convert calls. Row: DRAFT-025.
// Green on apps/web first.
//
// The creator-bypass halves are bug: scenarios (guest edits and
// member/guest single-fetches of owned drafts 403 on the oracle); those
// scenarios live below once their linked issue exists.
import { test, expect } from "../fixtures";
import {
  ROLE,
  serverConvertDraftStatus,
  serverCreateDraft,
  serverCreateDraftStatus,
  serverDeleteDraftStatus,
  serverDraftNames,
  serverDraftsPage,
  serverFetchDraftStatus,
  serverIssueNames,
  serverPatchDraftStatus,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { draftsHarness, draftsOpenAs, draftsSeat } from "./support";

test(
  specTitle(["DRAFT-025"], "guests create and remove their own drafts"),
  { tag: specTags(["DRAFT-025"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d25");
    const { owner, workspaceSlug, projectId } = harness;
    const name = `D25 Guest ${harness.tag}`;
    // Guests outside the project cannot resolve its marker, so their rows
    // render nothing (NEWFRONT-305); seat the guest into the project so
    // the UI path is reachable.
    const guest = await draftsSeat(harness, ROLE.GUEST, "parity-d25-guest", ROLE.GUEST);

    await test.step("a guest creates a draft through the API", async () => {
      const created = await serverCreateDraftStatus(workspaceSlug, guest.cookie, { name, project_id: projectId });
      expect(created.status).toBe(201);
    });

    await test.step("the guest removes it through the UI", async () => {
      await draftsOpenAs(driver, harness, guest);
      await expect.poll(() => driver.draftRowNames(), { timeout: 60_000 }).toContain(name);
      await driver.draftRowMenuClick(name, "delete");
      await driver.confirmDraftDelete();
      await expect.poll(() => driver.toastText(), { timeout: 30_000 }).toContain("Draft deleted");
      await expect.poll(() => serverDraftNames(workspaceSlug, guest.cookie), { timeout: 60_000 }).not.toContain(name);
    });

    await test.step("the owner's drafts are untouched", async () => {
      expect(await serverDraftNames(workspaceSlug, owner.cookie)).not.toContain(name);
    });
  }
);

test(
  specTitle(["DRAFT-025"], "members edit their own drafts inline and publish them"),
  { tag: specTags(["DRAFT-025"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d25b");
    const { workspaceSlug, projectId } = harness;
    const member = await draftsSeat(harness, ROLE.MEMBER, "parity-d25-member", ROLE.MEMBER);
    const name = `D25 Member ${harness.tag}`;
    const draft = await serverCreateDraft(workspaceSlug, member.cookie, { name, project_id: projectId });

    await test.step("a member changes their own draft inline", async () => {
      await draftsOpenAs(driver, harness, member);
      await expect.poll(() => driver.draftPriorityText(name), { timeout: 60_000 }).toBe("None");
      await driver.draftOpenPriorityPicker(name);
      await driver.pickerPick("Medium");
      await expect.poll(() => driver.draftPriorityText(name), { timeout: 30_000 }).toBe("Medium");
      // Members cannot single-fetch even owned drafts (NEWFRONT-299), so
      // verify through the owner-scoped list both roles can read.
      await expect
        .poll(
          async () =>
            (await serverDraftsPage(workspaceSlug, member.cookie, "")).drafts.find((row) => row.id === draft.id)
              ?.priority,
          { timeout: 30_000 }
        )
        .toBe("medium");
    });

    await test.step("a member publishes their own draft", async () => {
      // The convert payload carries the draft's fields client-side, like
      // the UI publish does; success creates the issue (201).
      const attempt = await serverConvertDraftStatus(workspaceSlug, draft.id, member.cookie, { name });
      expect(attempt.status).toBe(201);
      await expect
        .poll(() => serverIssueNames(workspaceSlug, projectId, member.cookie), { timeout: 60_000 })
        .toContain(name);
      expect(await serverDraftNames(workspaceSlug, member.cookie)).not.toContain(name);
    });
  }
);

test(
  specTitle(["DRAFT-025"], "the server refuses cross-user writes and guest converts"),
  { tag: specTags(["DRAFT-025"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d25c");
    const { owner, workspaceSlug, projectId } = harness;
    const name = `D25 Foreign ${harness.tag}`;
    const draft = await serverCreateDraft(workspaceSlug, owner.cookie, { name, project_id: projectId });
    const member = await draftsSeat(harness, ROLE.MEMBER, "parity-d25c-member", ROLE.MEMBER);
    const guest = await draftsSeat(harness, ROLE.GUEST, "parity-d25c-guest");
    const guestOwn = await serverCreateDraft(workspaceSlug, guest.cookie, {
      name: `D25c Guest Own ${harness.tag}`,
      project_id: projectId,
    });

    await test.step("cross-user deletes are refused and the draft survives", async () => {
      expect((await serverDeleteDraftStatus(workspaceSlug, draft.id, member.cookie)).status).toBe(403);
      expect((await serverDeleteDraftStatus(workspaceSlug, draft.id, guest.cookie)).status).toBe(403);
      expect(await serverDraftNames(workspaceSlug, owner.cookie)).toContain(name);
    });

    await test.step("a guest convert of their own draft is refused", async () => {
      expect((await serverConvertDraftStatus(workspaceSlug, guestOwn.id, guest.cookie, {})).status).toBe(403);
      expect(await serverDraftNames(workspaceSlug, guest.cookie)).toContain(guestOwn.name);
      expect(await serverIssueNames(workspaceSlug, projectId, owner.cookie)).not.toContain(guestOwn.name);
    });

    await test.step("single-fetch of another user's draft is refused", async () => {
      expect((await serverFetchDraftStatus(workspaceSlug, draft.id, member.cookie)).status).toBe(403);
      expect((await serverFetchDraftStatus(workspaceSlug, draft.id, guest.cookie)).status).toBe(403);
    });

    await test.step("the member never sees the foreign draft", async () => {
      await draftsOpenAs(driver, harness, member);
      expect(await driver.draftRowNames()).not.toContain(name);
    });
  }
);

// The update and retrieve actions declare a creator bypass but point it
// at work-item rows, so it never matches a draft id; DRAFT-025 requires
// admin-or-creator. Pinned here until NEWFRONT-299 lands the fix.
test(
  specTitle(["DRAFT-025"], "bug: creator bypass refuses owned-draft edits and fetches (NEWFRONT-299)"),
  { tag: specTags(["DRAFT-025"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d25bug");
    const { workspaceSlug, projectId } = harness;
    const guest = await draftsSeat(harness, ROLE.GUEST, "parity-d25bug-guest", ROLE.GUEST);
    const member = await draftsSeat(harness, ROLE.MEMBER, "parity-d25bug-member", ROLE.MEMBER);
    const guestDraft = await serverCreateDraft(workspaceSlug, guest.cookie, {
      name: `D25bug Guest ${harness.tag}`,
      project_id: projectId,
    });
    const memberDraft = await serverCreateDraft(workspaceSlug, member.cookie, {
      name: `D25bug Member ${harness.tag}`,
      project_id: projectId,
    });

    await test.step("owned edits and fetches are refused and nothing changes", async () => {
      expect(
        (await serverPatchDraftStatus(workspaceSlug, guestDraft.id, guest.cookie, { priority: "high" })).status
      ).toBe(403);
      expect((await serverFetchDraftStatus(workspaceSlug, memberDraft.id, member.cookie)).status).toBe(403);
      expect((await serverFetchDraftStatus(workspaceSlug, guestDraft.id, guest.cookie)).status).toBe(403);
      expect(await serverDraftNames(workspaceSlug, guest.cookie)).toContain(guestDraft.name);
      expect(await serverDraftNames(workspaceSlug, member.cookie)).toContain(memberDraft.name);
    });

    await test.step("the owners still see their rows", async () => {
      await draftsOpenAs(driver, harness, guest);
      expect(await driver.draftRowNames()).toContain(guestDraft.name);
    });
  }
);
