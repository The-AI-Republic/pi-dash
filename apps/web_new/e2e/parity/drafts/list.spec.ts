// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-32): the drafts list — owner scoping with
// newest-first order, the count chip, row identity, and the
// workspace-scoped route with no per-draft address. Rows: DRAFT-001,
// DRAFT-002, DRAFT-007, DRAFT-024. Green on apps/web first.
import { test, expect } from "../fixtures";
import { ROLE, browserSessionCookies, serverCreateDraft, serverDraftsPage } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { draftsHarness, draftsOpenAs, draftsSeat } from "./support";

test(
  specTitle(["DRAFT-001"], "drafts list shows only the viewer's drafts, newest first"),
  { tag: specTags(["DRAFT-001"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d1");
    const { owner, workspaceSlug, projectId } = harness;
    const first = `D1 First ${harness.tag}`;
    const second = `D1 Second ${harness.tag}`;
    const foreign = `D1 Foreign ${harness.tag}`;

    await serverCreateDraft(workspaceSlug, owner.cookie, { name: first, project_id: projectId });
    await serverCreateDraft(workspaceSlug, owner.cookie, { name: second, project_id: projectId });
    const member = await draftsSeat(harness, ROLE.MEMBER, "parity-d1-member");
    await serverCreateDraft(workspaceSlug, member.cookie, { name: foreign, project_id: projectId });

    await test.step("the owner sees only their own drafts, newest first", async () => {
      await draftsOpenAs(driver, harness);
      await expect.poll(() => driver.draftRowNames(), { timeout: 60_000 }).toEqual([second, first]);
    });

    await test.step("the member sees only theirs", async () => {
      await draftsOpenAs(driver, harness, member);
      await expect.poll(() => driver.draftRowNames(), { timeout: 60_000 }).toEqual([foreign]);
    });

    await test.step("the server agrees with both screens", async () => {
      const own = await serverDraftsPage(workspaceSlug, owner.cookie, "");
      expect(own.drafts.map((draft) => draft.name)).toEqual([second, first]);
      const theirs = await serverDraftsPage(workspaceSlug, member.cookie, "");
      expect(theirs.drafts.map((draft) => draft.name)).toEqual([foreign]);
    });
  }
);

test(
  specTitle(["DRAFT-002"], "count chip hides when empty and matches the server total"),
  { tag: specTags(["DRAFT-002"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d2");
    const { owner, workspaceSlug, projectId } = harness;

    await test.step("no chip on an empty screen", async () => {
      await draftsOpenAs(driver, harness);
      await expect.poll(() => driver.draftsCountChip(), { timeout: 60_000 }).toBeNull();
    });

    await test.step("the chip matches the server total once drafts exist", async () => {
      await serverCreateDraft(workspaceSlug, owner.cookie, { name: `D2 One ${harness.tag}`, project_id: projectId });
      await serverCreateDraft(workspaceSlug, owner.cookie, { name: `D2 Two ${harness.tag}`, project_id: projectId });
      await draftsOpenAs(driver, harness);
      await expect.poll(() => driver.draftsCountChip(), { timeout: 60_000 }).toBe("2");
      const page = await serverDraftsPage(workspaceSlug, owner.cookie, "");
      expect(page.total).toBe(2);
    });
  }
);

test(
  specTitle(["DRAFT-007"], "each row shows its project marker and title"),
  { tag: specTags(["DRAFT-007"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d7");
    const { owner, workspaceSlug, projectId, projectIdentifier } = harness;
    const name = `D7 Row ${harness.tag}`;
    await serverCreateDraft(workspaceSlug, owner.cookie, { name, project_id: projectId });

    await test.step("the row carries the project marker and the title", async () => {
      await draftsOpenAs(driver, harness);
      await expect.poll(() => driver.draftRowNames(), { timeout: 60_000 }).toContain(name);
      expect(await driver.draftRowProjectMarker(name)).toBe(projectIdentifier);
    });
  }
);

// The row's type-identifier component renders an empty fragment, so rows
// never show what kind of item the draft will become; DRAFT-007 requires
// the mark. Pinned here until NEWFRONT-301 lands the fix.
test(
  specTitle(["DRAFT-007"], "bug: rows carry no work-type mark (NEWFRONT-301)"),
  { tag: specTags(["DRAFT-007"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d7bug");
    const { owner, workspaceSlug, projectId } = harness;
    const name = `D7 Untyped ${harness.tag}`;
    await serverCreateDraft(workspaceSlug, owner.cookie, { name, project_id: projectId });

    await test.step("the marker area holds only the project button", async () => {
      await draftsOpenAs(driver, harness);
      await expect.poll(() => driver.draftRowNames(), { timeout: 60_000 }).toContain(name);
      expect(await driver.draftRowTypeMarkPresent(name)).toBe(false);
    });
  }
);

test(
  specTitle(["DRAFT-024"], "workspace drafts route renders with breadcrumb and title; no per-draft address"),
  { tag: specTags(["DRAFT-024"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d24");
    const { owner, workspaceSlug, projectId } = harness;
    const name = `D24 Row ${harness.tag}`;
    const draft = await serverCreateDraft(workspaceSlug, owner.cookie, { name, project_id: projectId });

    await test.step("the route renders the list with breadcrumb and title", async () => {
      await draftsOpenAs(driver, harness);
      expect(await driver.draftsPageTitle()).toBe("Workspace Draft");
      expect(await driver.draftsHeaderText()).toContain("Drafts");
      await expect.poll(() => driver.draftRowNames(), { timeout: 60_000 }).toContain(name);
    });

    await test.step("no URL opens the single draft", async () => {
      await driver.openAuthenticated(`/${workspaceSlug}/drafts/${draft.id}`, browserSessionCookies(owner));
      expect(await driver.currentPath()).toContain(`/drafts/${draft.id}`);
      // A single-draft view would render exactly this draft's editor; the
      // unknown route renders the app's not-found screen instead, with no
      // draft rows and no sign of the draft.
      await expect.poll(() => driver.pageTextContains("cannot be found"), { timeout: 60_000 }).toBe(true);
      expect(await driver.draftBlockCount()).toBe(0);
      expect(await driver.pageTextContains(name)).toBe(false);
    });
  }
);
