// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Palette unauthorized-entry hiding (NEWFRONT-127). Row: SHELL-092. Entries
// the caller may not use are hidden entirely, never disabled: a guest sees
// no project-scoped creation entries on the issues list ("New workspace" is
// instance-flag-gated and stays visible), and the work-item contextual group
// on an archived item keeps only its always-visible copy entries while the
// edit-gated entries stay hidden. (No view-but-not-edit project role exists
// — project guests cannot list issues at all — so the archived item, whose
// edit gate also closes, carries the contextual half of the row.)
import { test, expect } from "../../fixtures";
import {
  createServerState,
  deleteServerState,
  patchServerIssue,
  serverArchiveIssue,
  serverCreateIssue,
  serverDeleteIssue,
  serverEnsureProjectGuest,
  serverIssueDetail,
  serverStates,
  serverUnarchiveIssue,
  signInSession,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

test.describe("palette unauthorized-entry hiding", () => {
  test(
    specTitle(["SHELL-092"], "a guest sees no creation entries on the issues list"),
    { tag: specTags(["SHELL-092"]) },
    async ({ driver, seed }) => {
      test.setTimeout(300_000);
      expect(seed.guestEmail).toBeTruthy();
      expect(seed.guestPassword).toBeTruthy();
      const guestEmail = seed.guestEmail as string;
      const guestPassword = seed.guestPassword as string;

      // The guest reads the project shell (but no issues) only while its
      // project membership is active — the seed's intended state, ensured
      // idempotently here so the scenario heals drift instead of flaking.
      const ownerSession = await signInSession(seed.email, seed.password);
      await serverEnsureProjectGuest(seed.workspaceSlug, seed.projectId, guestEmail, ownerSession);
      await driver.openEntry();
      await driver.signInWithPassword(guestEmail, guestPassword);
      // Guests get no "Add work item" affordance, so the settled-list opener
      // (which waits for it) never resolves for them; settle on the project
      // name instead.
      await driver.goToPath(`/${seed.workspaceSlug}/projects/${seed.projectId}/issues`);
      await expect.poll(() => driver.hasVisibleText(seed.projectName), { timeout: 120_000 }).toBe(true);
      await driver.pressPaletteOpenChord();
      await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);
      const titles = await driver.paletteCommandTitles();
      // "New workspace" is gated by the instance creation flag, not by the
      // caller's project permissions, so it legitimately renders for guests;
      // the row's hiding claim covers the project-scoped creation entries.
      expect(titles.filter((t) => t.startsWith("New ") && t !== "New workspace")).toEqual([]);
      // Sanity: always-visible entries still render for the guest.
      expect(await driver.paletteHasCommand("Open keyboard shortcuts")).toBe(true);
    }
  );

  test(
    specTitle(["SHELL-092"], "an archived item keeps only copy entries even for the owner"),
    { tag: specTags(["SHELL-092"]) },
    async ({ driver, seed }) => {
      test.setTimeout(300_000);
      await test.step("archive an issue and prove the palette keeps only copy entries", async () => {
        const session = await signInSession(seed.email, seed.password);
        // Archiving requires a closed state group; reuse the project's
        // completed state when one exists, else mint and clean up our own.
        const states = await serverStates(seed.workspaceSlug, seed.projectId, session);
        let done = states.find((state) => state.group === "completed");
        let ownState = false;
        if (!done) {
          done = await createServerState(seed.workspaceSlug, seed.projectId, session, {
            name: `Parity done ${Date.now()}`,
            group: "completed",
            color: "#0E9F6E",
          });
          ownState = true;
        }
        const name = `Parity archived ${Date.now()}`;
        const issueId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, name);
        const detail = await serverIssueDetail(seed.workspaceSlug, seed.projectId, issueId, session);
        const key = `${detail.project_identifier}-${detail.sequence_id}`;
        let archived = false;
        try {
          await patchServerIssue(seed.workspaceSlug, seed.projectId, issueId, session, { state_id: done.id });
          await serverArchiveIssue(seed.workspaceSlug, seed.projectId, issueId, session);
          archived = true;
          await driver.openEntry();
          await driver.signInWithPassword(seed.email, seed.password);
          await driver.openBrowseWorkItem(seed.workspaceSlug, key);
          await expect.poll(() => driver.hasVisibleText(name), { timeout: 120_000 }).toBe(true);
          await driver.pressPaletteOpenChord();
          await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);
          await expect.poll(() => driver.paletteGroupHeadings()).toContain("Work item actions");
          expect(await driver.paletteHasCommand("Copy URL")).toBe(true);
          expect(await driver.paletteHasCommand("Copy ID")).toBe(true);
          expect(await driver.paletteHasCommand("Change state")).toBe(false);
          expect(await driver.paletteHasCommand("Change priority")).toBe(false);
          expect(await driver.paletteHasCommand("Assign to")).toBe(false);
          expect(await driver.paletteHasCommand("Delete")).toBe(false);
        } finally {
          // Unarchive only what archiving actually archived: a blind
          // unarchive 404s and would mask the real failure.
          if (archived) await serverUnarchiveIssue(seed.workspaceSlug, seed.projectId, issueId, session);
          await serverDeleteIssue(seed.workspaceSlug, seed.projectId, issueId, session);
          if (ownState) await deleteServerState(seed.workspaceSlug, seed.projectId, done.id, session);
        }
      });
    }
  );
});
