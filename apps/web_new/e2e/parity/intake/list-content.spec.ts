// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-256): intake list content — per-row content,
// cursor paging, the filter panel + chips, quick/explicit date ranges, and
// the order menu. Rows: INT-006, INT-007, INT-009, INT-010, INT-011.
//
// Observed behavior notes: rows are links keyed by nested issue id; pending
// rows render no status chip; the priority glyph always renders (its tooltip
// carries the key, including "none"); four or more labels collapse to a
// count pill; the author avatar gives way to the intake identity only for
// authors at the intake system address.
import { test, expect } from "../fixtures";
import {
  answerSingleInvitation,
  createWorkspaceInvites,
  deleteInvitation,
  fetchMe,
  parityProjectIdentifier,
  serverAddProjectMembers,
  serverCleanupProject,
  serverCreateLabel,
  serverCreateProjectWithFlags,
  serverIntakeListAll,
  serverIntakeListCreate,
  serverIntakeListDetail,
  serverIntakeListPage,
  serverIntakeListPatch,
  serverProject,
  sessionBrowserCookies,
  signInSession,
  signUpUser,
  workspaceInvitations,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["INT-006"], "list rows show id, title, date, priority, labels and the author avatar"),
  { tag: specTags(["INT-006"]) },
  async ({ driver, seed }) => {
    test.setTimeout(600_000);
    const tag = `NF256 row ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(seed.mentionMember!.email, seed.mentionMember!.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N256"),
      { inboxView: true },
      session
    );
    try {
      await serverAddProjectMembers(
        seed.workspaceSlug,
        projectId,
        [{ memberId: seed.mentionMember!.id, role: 15 }],
        session
      );
      const labelNames = [`${tag} LA`, `${tag} LB`, `${tag} LC`, `${tag} LD`];
      const labels = [];
      for (const [index, name] of labelNames.entries()) {
        labels.push(
          await serverCreateLabel(
            seed.workspaceSlug,
            projectId,
            name,
            ["#ff0000", "#00ff00", "#0000ff", "#ff00ff"][index]!,
            session
          )
        );
      }
      const urgent = await serverIntakeListCreate(seed.workspaceSlug, projectId, `${tag} urgent`, session, {
        priority: "urgent",
        labelIds: [labels[0]!.id],
      });
      const plain = await serverIntakeListCreate(seed.workspaceSlug, projectId, `${tag} plain`, session, {
        labelIds: labels.map((label) => label.id),
      });
      const memberRow = await serverIntakeListCreate(seed.workspaceSlug, projectId, `${tag} member`, memberSession, {
        priority: "low",
      });
      const declined = await serverIntakeListCreate(seed.workspaceSlug, projectId, `${tag} declined`, session);
      await serverIntakeListPatch(seed.workspaceSlug, projectId, declined.issueId, { status: -1 }, session);
      const project = await serverProject(seed.workspaceSlug, projectId, session);

      await driver.page.context().addCookies(sessionBrowserCookies(session));
      await driver.intakeListBeginRequestSpy();
      await driver.intakeListOpen(seed.workspaceSlug, projectId, "open");
      await driver.intakeListWaitForRowCount(3);

      await test.step("pending rows render with titles and no status chip", async () => {
        expect(await driver.intakeListRowIds()).toHaveLength(3);
        expect(await driver.intakeListRowTitle(urgent.issueId)).toBe(`${tag} urgent`);
        expect(await driver.intakeListRowTitle(plain.issueId)).toBe(`${tag} plain`);
        expect(await driver.intakeListRowTitle(memberRow.issueId)).toBe(`${tag} member`);
        expect(await driver.intakeListRowStatusChip(urgent.issueId)).toBe(null);
        expect(await driver.intakeListRowStatusChip(plain.issueId)).toBe(null);
        expect(await driver.intakeListRowStatusChip(memberRow.issueId)).toBe(null);
      });

      await test.step("rows carry the project-scoped id label", async () => {
        for (const row of [urgent, plain, memberRow]) {
          const detail = await serverIntakeListDetail(seed.workspaceSlug, projectId, row.issueId, session);
          expect(await driver.intakeListRowIdLabel(row.issueId)).toBe(`${project.identifier}-${detail.sequenceId}`);
        }
      });

      await test.step("created dates render with an elaborating tooltip", async () => {
        for (const row of [urgent, plain, memberRow]) {
          const text = await driver.intakeListRowCreatedText(row.issueId);
          expect(text.length).toBeGreaterThan(0);
          const tooltip = await driver.intakeListRowCreatedTooltip(row.issueId);
          expect(tooltip.heading.length).toBeGreaterThan(0);
          expect(tooltip.content).toContain(text);
        }
      });

      await test.step("priority markers present the priority key (tooltip never opens: NEWFRONT-273)", async () => {
        expect(await driver.intakeListRowPriority(urgent.issueId)).toBe("urgent");
        expect(await driver.intakeListRowPriority(memberRow.issueId)).toBe("low");
        expect(await driver.intakeListRowPriority(plain.issueId)).toBe("none");
      });

      await test.step("labels render singly and collapse to a count at four", async () => {
        expect(await driver.intakeListRowLabels(urgent.issueId)).toEqual([labelNames[0]]);
        const collapsed = await driver.intakeListRowLabels(plain.issueId);
        expect(collapsed).toHaveLength(1);
        expect(collapsed[0]).toContain("4");
        expect(collapsed[0]).toContain("label");
        expect(await driver.intakeListRowLabels(memberRow.issueId)).toEqual([]);
      });

      await test.step("member-authored rows show the member avatar", async () => {
        expect(await driver.intakeListRowAvatarKind(urgent.issueId)).toBe("member");
        expect(await driver.intakeListRowAvatarKind(memberRow.issueId)).toBe("member");
      });

      await test.step("the server agrees on status, priority, labels and authors", async () => {
        const owner = await fetchMe(session);
        const urgentDetail = await serverIntakeListDetail(seed.workspaceSlug, projectId, urgent.issueId, session);
        expect(urgentDetail.status).toBe(-2);
        expect(urgentDetail.priority).toBe("urgent");
        expect(urgentDetail.labelIds).toEqual([labels[0]!.id]);
        expect(urgentDetail.createdBy).toBe(owner.id);
        const plainDetail = await serverIntakeListDetail(seed.workspaceSlug, projectId, plain.issueId, session);
        expect(plainDetail.priority).toBe("none");
        expect(plainDetail.labelIds).toHaveLength(4);
        const memberDetail = await serverIntakeListDetail(seed.workspaceSlug, projectId, memberRow.issueId, session);
        expect(memberDetail.createdBy).toBe(seed.mentionMember!.id);
      });

      await test.step("a declined row shows its status chip on the closed tab", async () => {
        await driver.intakeListOpen(seed.workspaceSlug, projectId, "closed");
        expect(await driver.intakeListRowIds()).toEqual([declined.issueId]);
        expect(await driver.intakeListRowStatusChip(declined.issueId)).toBe("Declined");
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["INT-006"], "system-authored rows show the intake identity instead of a member avatar"),
  { tag: specTags(["INT-006"]) },
  async ({ driver, seed }) => {
    test.setTimeout(600_000);
    const tag = `NF256 ident ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N256"),
      { inboxView: true },
      session
    );
    try {
      const formsRow = await serverIntakeListCreate(seed.workspaceSlug, projectId, `${tag} forms`, session, {
        source: "FORMS",
      });
      const intakeEmail = "intake@example.com";
      try {
        await signUpUser(intakeEmail, "Parity-Intake-9x");
      } catch {
        // Retry-safe: the user persists on the scratch stack once created.
      }
      const intakeSession = await signInSession(intakeEmail, "Parity-Intake-9x");
      try {
        const pending = await workspaceInvitations(seed.workspaceSlug, session);
        for (const row of pending.filter((row) => row.email === intakeEmail)) {
          await deleteInvitation(seed.workspaceSlug, row.id, session);
        }
      } catch {
        // Best-effort cleanup of a previous attempt's pending invite.
      }
      try {
        const [invite] = await createWorkspaceInvites(seed.workspaceSlug, session, [{ email: intakeEmail, role: 15 }]);
        await answerSingleInvitation(seed.workspaceSlug, invite!.id, true, invite!.token, intakeSession);
      } catch {
        // Retry-safe: already a workspace member on the scratch stack.
      }
      const intakeMe = await fetchMe(intakeSession);
      await serverAddProjectMembers(seed.workspaceSlug, projectId, [{ memberId: intakeMe.id, role: 15 }], session);
      const systemRow = await serverIntakeListCreate(seed.workspaceSlug, projectId, `${tag} system`, intakeSession);

      await driver.page.context().addCookies(sessionBrowserCookies(session));
      await driver.intakeListOpen(seed.workspaceSlug, projectId, "open");
      await driver.intakeListWaitForRowCount(2);

      await test.step("a forms-source row by a member keeps the member avatar", async () => {
        expect(await driver.intakeListRowAvatarKind(formsRow.issueId)).toBe("member");
        const detail = await serverIntakeListDetail(seed.workspaceSlug, projectId, formsRow.issueId, session);
        expect(detail.source).toBe("FORMS");
      });

      await test.step("a system-authored row shows the intake identity", async () => {
        expect(await driver.intakeListRowAvatarKind(systemRow.issueId)).toBe("intake");
        const detail = await serverIntakeListDetail(seed.workspaceSlug, projectId, systemRow.issueId, session);
        expect(detail.createdBy).toBe(intakeMe.id);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["INT-007"], "scrolling pages ten rows at a time with placeholders and no duplicates"),
  { tag: specTags(["INT-007"]) },
  async ({ driver, seed }) => {
    test.setTimeout(600_000);
    const tag = `NF256 page ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N256"),
      { inboxView: true },
      session
    );
    try {
      for (let index = 0; index < 12; index += 1) {
        await serverIntakeListCreate(
          seed.workspaceSlug,
          projectId,
          `${tag} R${String(index).padStart(2, "0")}`,
          session
        );
      }
      const listParams = { status: "-2", order_by: "-issue__created_at", per_page: 10, cursor: "10:0:0" };
      const firstPage = await serverIntakeListPage(seed.workspaceSlug, projectId, session, listParams);
      expect(firstPage.rows).toHaveLength(10);
      expect(firstPage.nextPageResults).toBe(true);
      const secondPage = await serverIntakeListPage(seed.workspaceSlug, projectId, session, {
        ...listParams,
        cursor: firstPage.nextCursor,
      });
      expect(secondPage.rows).toHaveLength(2);
      const serverOrder = [...firstPage.rows, ...secondPage.rows].map((row) => row.issueId);
      expect(new Set(serverOrder).size).toBe(12);

      await driver.page.context().addCookies(sessionBrowserCookies(session));
      await driver.intakeListBeginRequestSpy();
      await driver.intakeListDelayNextListReads(5_000);
      await driver.intakeListOpen(seed.workspaceSlug, projectId, "open");
      await driver.intakeListWaitForRowCount(10);

      await test.step("the first page renders ten rows with placeholders below", async () => {
        expect(await driver.intakeListRowIds()).toHaveLength(10);
        expect(await driver.intakeListRowIds()).toEqual(firstPage.rows.map((row) => row.issueId));
        expect(await driver.intakeListSkeletonsVisible()).toBe(true);
      });

      await test.step("scrolling appends the next page without duplicates", async () => {
        await driver.intakeListScrollToBottom();
        await driver.intakeListWaitForRowCount(12);
        const ids = await driver.intakeListRowIds();
        expect(new Set(ids).size).toBe(12);
        expect(ids).toEqual(serverOrder);
        expect(await driver.intakeListSkeletonsVisible()).toBe(false);
      });

      await test.step("the UI pages with the same cursors as the API", async () => {
        const params = await driver.intakeListLastRequestParams();
        expect(params).not.toBe(null);
        expect(params!["per_page"]).toBe("10");
        expect(params!["cursor"]).toBe(firstPage.nextCursor);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["INT-007"], "bug: NEWFRONT-269 the header shows no syncing hint while a page fetches"),
  { tag: specTags(["INT-007"]) },
  async ({ driver, seed }) => {
    test.setTimeout(600_000);
    const tag = `NF256 sync ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N256"),
      { inboxView: true },
      session
    );
    try {
      for (let index = 0; index < 12; index += 1) {
        await serverIntakeListCreate(
          seed.workspaceSlug,
          projectId,
          `${tag} R${String(index).padStart(2, "0")}`,
          session
        );
      }
      await driver.page.context().addCookies(sessionBrowserCookies(session));
      await driver.intakeListOpen(seed.workspaceSlug, projectId, "open");
      await driver.intakeListWaitForRowCount(10);

      // The placeholders show while the next page is pending, but the header
      // hint the inventory expects never appears: nothing ever sets the
      // paged-loading loader state it reads (NEWFRONT-269).
      expect(await driver.intakeListSkeletonsVisible()).toBe(true);
      await driver.intakeListDelayNextListReads(5_000);
      await driver.intakeListScrollToBottom();
      await driver.page.waitForTimeout(1_500);
      expect(await driver.intakeListSkeletonsVisible()).toBe(true);
      expect(await driver.intakeListSyncingVisible()).toBe(false);
      await driver.intakeListWaitForRowCount(12);
      expect(await driver.intakeListSkeletonsVisible()).toBe(false);
      expect(await driver.intakeListSyncingVisible()).toBe(false);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["INT-009"], "filter panel searches sections and applies priority server-side"),
  { tag: specTags(["INT-009"]) },
  async ({ driver, seed }) => {
    test.setTimeout(600_000);
    const tag = `NF256 filt ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(seed.mentionMember!.email, seed.mentionMember!.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N256"),
      { inboxView: true },
      session
    );
    try {
      await serverAddProjectMembers(
        seed.workspaceSlug,
        projectId,
        [{ memberId: seed.mentionMember!.id, role: 15 }],
        session
      );
      const urgent = await serverIntakeListCreate(seed.workspaceSlug, projectId, `${tag} U`, session, {
        priority: "urgent",
      });
      await serverIntakeListCreate(seed.workspaceSlug, projectId, `${tag} H`, session, { priority: "high" });
      await serverIntakeListCreate(seed.workspaceSlug, projectId, `${tag} M`, memberSession, { priority: "low" });

      await driver.page.context().addCookies(sessionBrowserCookies(session));
      await driver.intakeListBeginRequestSpy();
      await driver.intakeListOpen(seed.workspaceSlug, projectId, "open");
      await driver.intakeListWaitForRowCount(3);

      await test.step("the panel lists all seven filter sections", async () => {
        expect(await driver.intakeListFilterSections()).toEqual([
          "status",
          "priority",
          "assignees",
          "createdBy",
          "labels",
          "createdAt",
          "updatedAt",
        ]);
      });

      await test.step("searching narrows entries within each section", async () => {
        await driver.intakeListFilterSearch("urg");
        expect(await driver.intakeListFilterOptions("priority")).toEqual(["Urgent"]);
        expect(await driver.intakeListFilterOptions("status")).toEqual([]);
        await driver.intakeListFilterSearch("");
        expect(await driver.intakeListFilterOptions("priority")).toEqual(["Urgent", "High", "Medium", "Low", "None"]);
        expect(await driver.intakeListFilterOptions("status")).toEqual(["Pending", "Snoozed"]);
      });

      await test.step("picking a priority narrows the list server-side", async () => {
        await driver.intakeListFilterPick("priority", "Urgent");
        await expect.poll(() => driver.intakeListRowIds(), { timeout: 30_000 }).toEqual([urgent.issueId]);
        expect(await driver.intakeListFilterChecked("priority", "Urgent")).toBe(true);
        expect(await driver.intakeListFilterSectionCount("priority")).toBe(1);
        expect(await driver.intakeListChips()).toContainEqual({ key: "priority", values: ["Urgent"] });
        const params = await driver.intakeListLastRequestParams();
        expect(params!["priority"]).toBe("urgent");
        const serverRows = await serverIntakeListAll(seed.workspaceSlug, projectId, session, {
          status: "-2",
          priority: "urgent",
        });
        expect(serverRows.map((row) => row.issueId)).toEqual([urgent.issueId]);
      });

      await test.step("unpicking restores the full list", async () => {
        await driver.intakeListFilterPick("priority", "Urgent");
        await expect.poll(() => driver.intakeListRowIds(), { timeout: 30_000 }).toHaveLength(3);
        expect(await driver.intakeListFilterSectionCount("priority")).toBe(0);
        const chips = await driver.intakeListChips();
        expect(chips.find((chip) => chip.key === "priority")).toBe(undefined);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["INT-009"], "assignee, author, label and status filters narrow and clear through chips"),
  { tag: specTags(["INT-009"]) },
  async ({ driver, seed }) => {
    test.setTimeout(600_000);
    const tag = `NF256 chip ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(seed.mentionMember!.email, seed.mentionMember!.password);
    const memberName = seed.mentionMember!.displayName;
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N256"),
      { inboxView: true },
      session
    );
    try {
      await serverAddProjectMembers(
        seed.workspaceSlug,
        projectId,
        [{ memberId: seed.mentionMember!.id, role: 15 }],
        session
      );
      const labelA = await serverCreateLabel(seed.workspaceSlug, projectId, `${tag} LA`, "#ff0000", session);
      const owned = await serverIntakeListCreate(seed.workspaceSlug, projectId, `${tag} owned`, session, {
        labelIds: [labelA.id],
      });
      const assigned = await serverIntakeListCreate(seed.workspaceSlug, projectId, `${tag} assigned`, session);
      await serverIntakeListPatch(
        seed.workspaceSlug,
        projectId,
        assigned.issueId,
        { issue: { assignee_ids: [seed.mentionMember!.id] } },
        session
      );
      const authored = await serverIntakeListCreate(seed.workspaceSlug, projectId, `${tag} authored`, memberSession);

      await driver.page.context().addCookies(sessionBrowserCookies(session));
      await driver.intakeListBeginRequestSpy();
      await driver.intakeListOpen(seed.workspaceSlug, projectId, "open");
      await driver.intakeListWaitForRowCount(3);
      const baseline = await driver.intakeListRowIds();
      expect(baseline).toHaveLength(3);

      await test.step("assignee filter narrows to the member's row and clears", async () => {
        await driver.intakeListFilterPick("assignees", memberName);
        await expect.poll(() => driver.intakeListRowIds(), { timeout: 30_000 }).toEqual([assigned.issueId]);
        await expect
          .poll(() => driver.intakeListChips(), { timeout: 30_000 })
          .toContainEqual({ key: "assignees", values: [memberName] });
        expect((await driver.intakeListLastRequestParams())!["assignees"]).toBe(seed.mentionMember!.id);
        await driver.intakeListChipRemove("assignees", memberName);
        await expect.poll(() => driver.intakeListRowIds(), { timeout: 30_000 }).toHaveLength(3);
      });

      await test.step("created-by filter narrows to the author's row and clears", async () => {
        await driver.intakeListFilterPick("createdBy", memberName);
        await expect.poll(() => driver.intakeListRowIds(), { timeout: 30_000 }).toEqual([authored.issueId]);
        await expect
          .poll(() => driver.intakeListChips(), { timeout: 30_000 })
          .toContainEqual({ key: "createdBy", values: [memberName] });
        expect((await driver.intakeListLastRequestParams())!["created_by"]).toBe(seed.mentionMember!.id);
        await driver.intakeListChipRemove("createdBy", memberName);
        await expect.poll(() => driver.intakeListRowIds(), { timeout: 30_000 }).toHaveLength(3);
      });

      await test.step("label filter narrows to the tagged row and clears", async () => {
        await driver.intakeListFilterPick("labels", `${tag} LA`);
        await expect.poll(() => driver.intakeListRowIds(), { timeout: 30_000 }).toEqual([owned.issueId]);
        await expect
          .poll(() => driver.intakeListChips(), { timeout: 30_000 })
          .toContainEqual({ key: "labels", values: [`${tag} LA`] });
        expect((await driver.intakeListLastRequestParams())!["labels"]).toBe(labelA.id);
        await driver.intakeListChipRemove("labels", `${tag} LA`);
        await expect.poll(() => driver.intakeListRowIds(), { timeout: 30_000 }).toHaveLength(3);
      });

      await test.step("status filter is multi-select with a pinned last value", async () => {
        expect(await driver.intakeListChips()).toContainEqual({ key: "status", values: ["Pending"] });
        await driver.intakeListFilterPick("status", "Snoozed");
        await expect
          .poll(() => driver.intakeListChips(), { timeout: 30_000 })
          .toContainEqual({
            key: "status",
            values: ["Pending", "Snoozed"],
          });
        expect(await driver.intakeListFilterSectionCount("status")).toBe(2);
        expect((await driver.intakeListLastRequestParams())!["status"]).toBe("-2,0");
        expect(await driver.intakeListRowIds()).toHaveLength(3);
        await driver.intakeListChipRemove("status", "Snoozed");
        await expect
          .poll(() => driver.intakeListChips(), { timeout: 30_000 })
          .toContainEqual({
            key: "status",
            values: ["Pending"],
          });
      });

      await test.step("clearing the last status is refused and keeps the list", async () => {
        await driver.intakeListFilterPick("status", "Pending");
        expect(await driver.intakeListFilterChecked("status", "Pending")).toBe(true);
        expect(await driver.intakeListRowIds()).toHaveLength(3);
        expect(await driver.intakeListChips()).toContainEqual({ key: "status", values: ["Pending"] });
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["INT-010"], "quick and explicit date ranges filter created and updated dates"),
  { tag: specTags(["INT-010"]) },
  async ({ driver, seed }) => {
    test.setTimeout(600_000);
    const tag = `NF256 date ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N256"),
      { inboxView: true },
      session
    );
    const todayISO = new Date().toISOString().slice(0, 10);
    const yesterdayISO = new Date(Date.now() - 86_400_000).toISOString().slice(0, 10);
    try {
      const first = await serverIntakeListCreate(seed.workspaceSlug, projectId, `${tag} one`, session);
      const second = await serverIntakeListCreate(seed.workspaceSlug, projectId, `${tag} two`, session);
      const third = await serverIntakeListCreate(seed.workspaceSlug, projectId, `${tag} three`, session);
      const allIds = [third.issueId, second.issueId, first.issueId];

      await driver.page.context().addCookies(sessionBrowserCookies(session));
      await driver.intakeListBeginRequestSpy();
      await driver.intakeListOpen(seed.workspaceSlug, projectId, "open");
      await driver.intakeListWaitForRowCount(3);

      await test.step("both date sections offer presets plus an explicit range", async () => {
        for (const key of ["createdAt", "updatedAt"] as const) {
          expect(await driver.intakeListFilterOptions(key)).toEqual([
            "Today",
            "Yesterday",
            "Last 7 days",
            "Last 30 days",
            "Custom",
          ]);
        }
      });

      await test.step("the today preset keeps today's rows on both dates", async () => {
        await driver.intakeListFilterPick("createdAt", "Today");
        await expect.poll(() => driver.intakeListRowIds(), { timeout: 30_000 }).toEqual(allIds);
        await expect
          .poll(() => driver.intakeListChips(), { timeout: 30_000 })
          .toContainEqual({ key: "createdAt", values: ["Today"] });
        const createdParams = (await driver.intakeListLastRequestParams())!["created_at"]!;
        expect(createdParams).toBe(`${todayISO};after,${todayISO};before`);
        const serverToday = await serverIntakeListAll(seed.workspaceSlug, projectId, session, {
          status: "-2",
          created_at: createdParams,
        });
        expect(serverToday.map((row) => row.issueId)).toEqual(allIds);
        await driver.intakeListChipClearGroup("createdAt");

        await driver.intakeListFilterPick("updatedAt", "Today");
        await expect.poll(() => driver.intakeListRowIds(), { timeout: 30_000 }).toEqual(allIds);
        await expect
          .poll(() => driver.intakeListChips(), { timeout: 30_000 })
          .toContainEqual({ key: "updatedAt", values: ["Today"] });
        const updatedParams = (await driver.intakeListLastRequestParams())!["updated_at"]!;
        expect(updatedParams).toBe(`${todayISO};after,${todayISO};before`);
        await driver.intakeListChipClearGroup("updatedAt");
      });

      await test.step("the yesterday preset excludes today's rows and toggles off", async () => {
        await driver.intakeListFilterPick("createdAt", "Yesterday");
        await expect.poll(() => driver.intakeListRowIds(), { timeout: 30_000 }).toHaveLength(0);
        await expect
          .poll(() => driver.intakeListChips(), { timeout: 30_000 })
          .toContainEqual({
            key: "createdAt",
            values: ["Yesterday"],
          });
        const yesterdayParams = (await driver.intakeListLastRequestParams())!["created_at"]!;
        expect(yesterdayParams).toBe(`${yesterdayISO};after,${yesterdayISO};before`);
        await driver.intakeListFilterPick("createdAt", "Yesterday");
        await expect.poll(() => driver.intakeListRowIds(), { timeout: 30_000 }).toEqual(allIds);
      });

      await test.step("the last-7 and last-30-day presets keep today's rows", async () => {
        for (const preset of ["Last 7 days", "Last 30 days"]) {
          await driver.intakeListFilterPick("createdAt", preset);
          await expect.poll(() => driver.intakeListRowIds(), { timeout: 30_000 }).toEqual(allIds);
          await expect
            .poll(() => driver.intakeListChips(), { timeout: 30_000 })
            .toContainEqual({
              key: "createdAt",
              values: [preset],
            });
          await driver.intakeListChipClearGroup("createdAt");
        }
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["INT-010"], "bug: NEWFRONT-274 explicit from/to ranges send an empty range and never narrow"),
  { tag: specTags(["INT-010"]) },
  async ({ driver, seed }) => {
    test.setTimeout(600_000);
    const tag = `NF256 range ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N256"),
      { inboxView: true },
      session
    );
    // A range that excludes every fixture row: the fixed app empties the
    // list, but the old app sends an empty pair and keeps all rows.
    const fromISO = new Date(Date.now() - 8 * 86_400_000).toISOString().slice(0, 10);
    const toISO = new Date(Date.now() - 7 * 86_400_000).toISOString().slice(0, 10);
    try {
      await serverIntakeListCreate(seed.workspaceSlug, projectId, `${tag} one`, session);
      await serverIntakeListCreate(seed.workspaceSlug, projectId, `${tag} two`, session);
      await serverIntakeListCreate(seed.workspaceSlug, projectId, `${tag} three`, session);

      await driver.page.context().addCookies(sessionBrowserCookies(session));
      await driver.intakeListBeginRequestSpy();
      await driver.intakeListOpen(seed.workspaceSlug, projectId, "open");
      await driver.intakeListWaitForRowCount(3);

      for (const key of ["createdAt", "updatedAt"] as const) {
        await test.step(`explicit range on ${key} chips correctly but sends an empty pair`, async () => {
          await driver.intakeListOpenCustomDate(key);
          await driver.intakeListCustomDateApply(fromISO, toISO);
          await expect
            .poll(async () => (await driver.intakeListChips()).find((chip) => chip.key === key)?.values ?? [], {
              timeout: 30_000,
            })
            .toHaveLength(2);
          const values = (await driver.intakeListChips()).find((chip) => chip.key === key)!.values;
          expect(values[0]).toMatch(/^After /);
          expect(values[1]).toMatch(/^Before /);
          expect((await driver.intakeListLastRequestParams())![key === "createdAt" ? "created_at" : "updated_at"]).toBe(
            ","
          );
          expect(await driver.intakeListRowIds()).toHaveLength(3);
          await driver.intakeListChipClearGroup(key);
          await expect.poll(() => driver.intakeListRowIds(), { timeout: 30_000 }).toHaveLength(3);
        });
      }
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["INT-011"], "order menu re-sorts by created, updated and id in both directions"),
  { tag: specTags(["INT-011"]) },
  async ({ driver, seed }) => {
    test.setTimeout(600_000);
    const tag = `NF256 ord ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N256"),
      { inboxView: true },
      session
    );
    try {
      const first = await serverIntakeListCreate(seed.workspaceSlug, projectId, `${tag} A`, session);
      const second = await serverIntakeListCreate(seed.workspaceSlug, projectId, `${tag} B`, session);
      const third = await serverIntakeListCreate(seed.workspaceSlug, projectId, `${tag} C`, session);
      await serverIntakeListPatch(
        seed.workspaceSlug,
        projectId,
        second.issueId,
        { issue: { priority: "high" } },
        session
      );
      const orderByFor = (field: "created" | "updated" | "id", direction: "asc" | "desc"): string => {
        const column =
          field === "id" ? "issue__sequence_id" : field === "updated" ? "issue__updated_at" : "issue__created_at";
        return direction === "desc" ? `-${column}` : column;
      };

      await driver.page.context().addCookies(sessionBrowserCookies(session));
      await driver.intakeListBeginRequestSpy();
      await driver.intakeListOpen(seed.workspaceSlug, projectId, "open");
      await driver.intakeListWaitForRowCount(3);

      await test.step("the default order is newest-created first and marked", async () => {
        expect(await driver.intakeListOrderState()).toEqual({ field: "created", direction: "desc" });
        const expected = await serverIntakeListAll(seed.workspaceSlug, projectId, session, {
          status: "-2",
          order_by: "-issue__created_at",
        });
        expect(await driver.intakeListRowIds()).toEqual(expected.map((row) => row.issueId));
      });

      for (const field of ["created", "updated", "id"] as const) {
        for (const direction of ["desc", "asc"] as const) {
          await test.step(`order by ${field} ${direction} re-sorts server-side and stays marked`, async () => {
            await driver.intakeListPickOrderField(field);
            await driver.intakeListPickOrderDirection(direction);
            const expected = await serverIntakeListAll(seed.workspaceSlug, projectId, session, {
              status: "-2",
              order_by: orderByFor(field, direction),
            });
            await expect
              .poll(() => driver.intakeListRowIds(), { timeout: 30_000 })
              .toEqual(expected.map((row) => row.issueId));
            expect(await driver.intakeListOrderState()).toEqual({ field, direction });
            expect((await driver.intakeListLastRequestParams())!["order_by"]).toBe(orderByFor(field, direction));
          });
        }
      }
      void first;
      void third;
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
