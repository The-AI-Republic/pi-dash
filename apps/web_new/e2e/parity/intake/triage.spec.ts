// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-259): intake triage actions — accept into the
// project, decline, snooze/un-snooze, mark as duplicate, delete. Rows:
// INT-020, INT-021, INT-022, INT-023, INT-025.
//
// Admin flows run on fresh inbox-view projects (two requests each, so the
// neighbour advance is deterministic); member-refused paths run on the
// seeded project, where the seed provides a plain project member. Every
// scenario asserts what the user sees plus the resulting server state.
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverCleanupInboxIssue,
  serverCleanupProject,
  serverCreateInboxIssue,
  serverCreateIssue,
  serverCreateProjectWithFlags,
  serverDeleteIssue,
  serverInboxIssues,
  serverIntakeTriageDeleteRaw,
  serverIntakeTriageDetail,
  serverIntakeTriagePatch,
  serverIntakeTriagePatchRaw,
  serverIntakeTriageSearchIssues,
  serverIntakeTriageWorkItemExists,
  serverIssueDetail,
  signInSessionRetry,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

// Intake status values as the server reports them (observed from the
// seeded stack; the UI maps them to Pending/Declined/Snoozed/Accepted/
// Duplicate).
const STATUS_PENDING = -2;
const STATUS_DECLINED = -1;
const STATUS_SNOOZED = 0;
const STATUS_ACCEPTED = 1;
const STATUS_DUPLICATE = 2;

test(
  specTitle(["INT-020"], "accept moves the request into the project and advances to a neighbour"),
  { tag: specTags(["INT-020"]) },
  async ({ driver, seed }) => {
    test.setTimeout(420_000);
    const tag = `NF259 accept ${Date.now()}`;
    const session = await signInSessionRetry(seed.email, seed.password);
    const identifier = parityProjectIdentifier("N259");
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      identifier,
      { inboxView: true },
      session
    );
    const first = await serverCreateInboxIssue(seed.workspaceSlug, projectId, `${tag} A`, session);
    const second = await serverCreateInboxIssue(seed.workspaceSlug, projectId, `${tag} B`, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.rulesOpenIntakeIssue(seed.workspaceSlug, projectId, first.issueId);
      await expect.poll(() => driver.rulesIntakeTriageVisible(), { timeout: 60_000 }).toBe(true);

      await test.step("the add-to-project dialog opens the work-item form", async () => {
        expect(await driver.intakeTriageAcceptVisible()).toBe(true);
        await driver.intakeTriageClickAccept();
        await expect.poll(() => driver.intakeTriageAcceptDialogVisible(), { timeout: 30_000 }).toBe(true);
        const text = await driver.intakeTriageAcceptDialogText();
        expect(text.heading).toContain("to project work items");
        expect(text.confirm).toBe("Add to project");
      });

      await test.step("cancelling leaves the request pending", async () => {
        await driver.intakeTriageAcceptDialogCancel();
        expect(await driver.intakeTriageAcceptDialogVisible()).toBe(false);
        expect(await driver.intakeTriageCurrentIssueId()).toBe(first.issueId);
        const stored = await serverIntakeTriageDetail(seed.workspaceSlug, projectId, first.issueId, session);
        expect(stored.status).toBe(STATUS_PENDING);
      });

      await test.step("confirming accepts, keeps the work item and advances focus", async () => {
        await driver.intakeTriageClickAccept();
        await driver.intakeTriageAcceptDialogConfirm();
        // Secondary INT-028 assertion: the URL tracks the move (owned by NEWFRONT-258).
        await expect.poll(() => driver.intakeTriageCurrentIssueId(), { timeout: 30_000 }).toBe(second.issueId);
        expect(await driver.intakeTriageCurrentTab()).toBe("open");
        const stored = await serverIntakeTriageDetail(seed.workspaceSlug, projectId, first.issueId, session);
        expect(stored.status).toBe(STATUS_ACCEPTED);
        expect(await serverIntakeTriageWorkItemExists(seed.workspaceSlug, projectId, first.issueId, session)).toBe(
          true
        );
      });

      await test.step("the triage controls are absent once the item is resolved", async () => {
        await driver.intakeTriageOpenIssueOnTab(seed.workspaceSlug, projectId, first.issueId, "closed");
        await expect.poll(() => driver.intakeTriageStatusChipText(), { timeout: 60_000 }).toBe("Accepted");
        expect(await driver.intakeTriageAcceptVisible()).toBe(false);
        expect(await driver.intakeTriageDeclineVisible()).toBe(false);
        expect(await driver.intakeTriageOverflowVisible()).toBe(false);
      });
    } finally {
      await serverCleanupInboxIssue(seed.workspaceSlug, projectId, first.issueId, session);
      await serverDeleteIssue(seed.workspaceSlug, projectId, first.issueId, session).catch(() => undefined);
      await serverCleanupInboxIssue(seed.workspaceSlug, projectId, second.issueId, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["INT-020", "INT-021"], "member accept and decline attempts are refused with a permission notice"),
  { tag: specTags(["INT-020", "INT-021"]) },
  async ({ driver, seed }) => {
    test.setTimeout(420_000);
    const member = seed.mentionMember;
    if (!member) throw new Error("[parity] seed has no member identity; rerun parity-up.sh from a clean checkout.");
    const tag = `NF259 refused ${Date.now()}`;
    const admin = await signInSessionRetry(seed.email, seed.password);
    const inbox = await serverCreateInboxIssue(seed.workspaceSlug, seed.projectId, tag, admin);
    try {
      await driver.rulesEnsureSignedIn(member.email, member.password, seed.workspaceSlug);
      await driver.rulesOpenIntakeIssue(seed.workspaceSlug, seed.projectId, inbox.issueId);
      await expect.poll(() => driver.rulesIntakeTriageVisible(), { timeout: 60_000 }).toBe(true);

      await test.step("accept is offered but refused", async () => {
        expect(await driver.intakeTriageAcceptVisible()).toBe(true);
        await driver.intakeTriageClickAccept();
        await expect
          .poll(() => driver.rulesLastToast(), { timeout: 15_000 })
          .toEqual({ title: "Permission denied", message: expect.stringContaining("admin") });
        expect(await driver.intakeTriageAcceptDialogVisible()).toBe(false);
      });

      await test.step("decline is offered but refused", async () => {
        expect(await driver.intakeTriageDeclineVisible()).toBe(true);
        await driver.intakeTriageClickDecline();
        await expect
          .poll(() => driver.rulesLastToast(), { timeout: 15_000 })
          .toEqual({ title: "Permission denied", message: expect.stringContaining("admin") });
        expect(await driver.intakeTriageDeclineDialogVisible()).toBe(false);
      });

      await test.step("the server row is unchanged", async () => {
        const stored = await serverIntakeTriageDetail(seed.workspaceSlug, seed.projectId, inbox.issueId, admin);
        expect(stored.status).toBe(STATUS_PENDING);
      });
    } finally {
      await serverCleanupInboxIssue(seed.workspaceSlug, seed.projectId, inbox.issueId, admin);
    }
  }
);

test(
  specTitle(["INT-021"], "decline confirms through an irreversible-action dialog and advances focus"),
  { tag: specTags(["INT-021"]) },
  async ({ driver, seed }) => {
    test.setTimeout(420_000);
    const tag = `NF259 decline ${Date.now()}`;
    const session = await signInSessionRetry(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N259"),
      { inboxView: true },
      session
    );
    const first = await serverCreateInboxIssue(seed.workspaceSlug, projectId, `${tag} A`, session);
    const second = await serverCreateInboxIssue(seed.workspaceSlug, projectId, `${tag} B`, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.rulesOpenIntakeIssue(seed.workspaceSlug, projectId, first.issueId);
      await expect.poll(() => driver.rulesIntakeTriageVisible(), { timeout: 60_000 }).toBe(true);

      await test.step("the dialog warns the action cannot be undone", async () => {
        await driver.intakeTriageClickDecline();
        await expect.poll(() => driver.intakeTriageDeclineDialogVisible(), { timeout: 30_000 }).toBe(true);
        const text = await driver.intakeTriageDeclineDialogText();
        expect(text.heading).toBe("Decline work item");
        expect(text.body).toContain("cannot be undone");
      });

      await test.step("cancelling leaves the request pending", async () => {
        await driver.intakeTriageDeclineCancel();
        expect(await driver.intakeTriageDeclineDialogVisible()).toBe(false);
        expect(await driver.intakeTriageCurrentIssueId()).toBe(first.issueId);
        const stored = await serverIntakeTriageDetail(seed.workspaceSlug, projectId, first.issueId, session);
        expect(stored.status).toBe(STATUS_PENDING);
      });

      await test.step("confirming declines and advances to the neighbour", async () => {
        await driver.intakeTriageClickDecline();
        await driver.intakeTriageDeclineConfirm();
        // Secondary INT-028 assertion: the URL tracks the move (owned by NEWFRONT-258).
        await expect.poll(() => driver.intakeTriageCurrentIssueId(), { timeout: 30_000 }).toBe(second.issueId);
        const stored = await serverIntakeTriageDetail(seed.workspaceSlug, projectId, first.issueId, session);
        expect(stored.status).toBe(STATUS_DECLINED);
      });
    } finally {
      await serverCleanupInboxIssue(seed.workspaceSlug, projectId, first.issueId, session);
      await serverCleanupInboxIssue(seed.workspaceSlug, projectId, second.issueId, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["INT-022"], "snooze sets a future date with a countdown chip; un-snooze clears it"),
  { tag: specTags(["INT-022"]) },
  async ({ driver, seed }) => {
    test.setTimeout(420_000);
    const tag = `NF259 snooze ${Date.now()}`;
    const session = await signInSessionRetry(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N259"),
      { inboxView: true },
      session
    );
    const first = await serverCreateInboxIssue(seed.workspaceSlug, projectId, `${tag} A`, session);
    const second = await serverCreateInboxIssue(seed.workspaceSlug, projectId, `${tag} B`, session);
    const daysAhead = 7;
    const target = new Date();
    target.setDate(target.getDate() + daysAhead);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.rulesOpenIntakeIssue(seed.workspaceSlug, projectId, first.issueId);
      await expect.poll(() => driver.rulesIntakeTriageVisible(), { timeout: 60_000 }).toBe(true);

      await test.step("the overflow menu offers snooze and the picker forbids past dates", async () => {
        const options = await driver.intakeTriageOverflowOptions();
        expect(options).toContain("Snooze");
        await driver.intakeTriageClickSnooze();
        expect(await driver.intakeTriageSnoozeDialogVisible()).toBe(true);
        expect(await driver.intakeTriageSnoozePastDisabled()).toBe(true);
      });

      await test.step("snoozing stores the date and advances focus", async () => {
        await driver.intakeTriageSnoozePickFuture(daysAhead);
        await driver.intakeTriageSnoozeConfirm();
        await expect.poll(() => driver.intakeTriageCurrentIssueId(), { timeout: 30_000 }).toBe(second.issueId);
        const stored = await serverIntakeTriageDetail(seed.workspaceSlug, projectId, first.issueId, session);
        expect(stored.status).toBe(STATUS_SNOOZED);
        expect(stored.snoozedTill).not.toBeNull();
        expect(new Date(stored.snoozedTill ?? "").toDateString()).toBe(target.toDateString());
      });

      await test.step("the snoozed request shows a days-remaining chip", async () => {
        // Snoozed rows leave the default open list; the status filter brings this one back.
        await driver.intakeTriageFilterOpen();
        await driver.intakeTriageFilterToggleStatus("Snoozed");
        await driver.intakeTriageFilterClose();
        await driver.intakeTriageListOpenIssue(`${tag} A`);
        await expect.poll(() => driver.intakeTriageStatusChipText(), { timeout: 60_000 }).toMatch(/^\d+ days? to go$/);
        const options = await driver.intakeTriageOverflowOptions();
        expect(options).toContain("Un snooze");
      });

      await test.step("un-snooze clears the date and restores pending", async () => {
        await driver.intakeTriageClickUnsnooze();
        await expect.poll(() => driver.intakeTriageCurrentIssueId(), { timeout: 30_000 }).toBe(second.issueId);
        const stored = await serverIntakeTriageDetail(seed.workspaceSlug, projectId, first.issueId, session);
        expect(stored.status).toBe(STATUS_PENDING);
        expect(stored.snoozedTill).toBeNull();
        await driver.intakeTriageListOpenIssue(`${tag} A`);
        await expect.poll(() => driver.intakeTriageStatusChipText(), { timeout: 60_000 }).toBe("Pending");
      });
    } finally {
      await serverCleanupInboxIssue(seed.workspaceSlug, projectId, first.issueId, session);
      await serverCleanupInboxIssue(seed.workspaceSlug, projectId, second.issueId, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["INT-022"], "bug: NEWFRONT-272 a lapsed snooze hides the status chip; intended passed styling"),
  { tag: specTags(["INT-022"]) },
  async ({ driver, seed }) => {
    test.setTimeout(420_000);
    const tag = `NF259 lapsed ${Date.now()}`;
    const session = await signInSessionRetry(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N259"),
      { inboxView: true },
      session
    );
    const inbox = await serverCreateInboxIssue(seed.workspaceSlug, projectId, `${tag} A`, session);
    const other = await serverCreateInboxIssue(seed.workspaceSlug, projectId, `${tag} B`, session);
    try {
      // Simulate the lapse deterministically: the picker forbids past
      // dates, so set one directly through the API.
      const past = new Date();
      past.setDate(past.getDate() - 1);
      const stored = await serverIntakeTriagePatch(
        seed.workspaceSlug,
        projectId,
        inbox.issueId,
        {
          status: STATUS_SNOOZED,
          snoozed_till: past.toISOString(),
        },
        session
      );
      expect(stored.status).toBe(STATUS_SNOOZED);

      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.rulesOpenIntakeIssue(seed.workspaceSlug, projectId, other.issueId);
      await expect.poll(() => driver.rulesIntakeTriageVisible(), { timeout: 60_000 }).toBe(true);
      // Snoozed rows leave the default open list; the status filter brings this one back.
      await driver.intakeTriageFilterOpen();
      await driver.intakeTriageFilterToggleStatus("Snoozed");
      await driver.intakeTriageFilterClose();
      await driver.intakeTriageListOpenIssue(`${tag} A`);
      await expect.poll(() => driver.rulesIntakeTriageVisible(), { timeout: 60_000 }).toBe(true);
      expect(await driver.intakeTriageStatusChipText()).toBeNull();
    } finally {
      await serverCleanupInboxIssue(seed.workspaceSlug, projectId, inbox.issueId, session);
      await serverCleanupInboxIssue(seed.workspaceSlug, projectId, other.issueId, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["INT-023"], "mark as duplicate links the request to a picked work item"),
  { tag: specTags(["INT-023"]) },
  async ({ driver, seed }) => {
    test.setTimeout(420_000);
    const tag = `NF259 dup ${Date.now()}`;
    const session = await signInSessionRetry(seed.email, seed.password);
    const identifier = parityProjectIdentifier("N259");
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      identifier,
      { inboxView: true },
      session
    );
    const inbox = await serverCreateInboxIssue(seed.workspaceSlug, projectId, `${tag} request`, session);
    const targetName = `${tag} target`;
    const targetId = await serverCreateIssue(seed.workspaceSlug, projectId, session, targetName);
    const target = await serverIssueDetail(seed.workspaceSlug, projectId, targetId, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.rulesOpenIntakeIssue(seed.workspaceSlug, projectId, inbox.issueId);
      await expect.poll(() => driver.rulesIntakeTriageVisible(), { timeout: 60_000 }).toBe(true);

      await test.step("the picker searches project work items", async () => {
        const options = await driver.intakeTriageOverflowOptions();
        expect(options).toContain("Mark as duplicate");
        await driver.intakeTriageClickMarkDuplicate();
        expect(await driver.intakeTriageDuplicateDialogVisible()).toBe(true);
        await driver.intakeTriageDuplicateSearch(targetName);
        await expect
          .poll(() => driver.intakeTriageDuplicateOptions(), { timeout: 30_000 })
          .toEqual(expect.arrayContaining([expect.stringContaining(targetName)]));
        const serverCandidates = await serverIntakeTriageSearchIssues(
          seed.workspaceSlug,
          projectId,
          targetName,
          session
        );
        expect(serverCandidates.map((c) => c.id)).toContain(targetId);
      });

      await test.step("the picker excludes the current request", async () => {
        await driver.intakeTriageDuplicateSearch(`${tag} request`);
        const options = await driver.intakeTriageDuplicateOptions();
        expect(options.join("\n")).not.toContain(`${tag} request`);
        const serverCandidates = await serverIntakeTriageSearchIssues(
          seed.workspaceSlug,
          projectId,
          `${tag} request`,
          session
        );
        expect(serverCandidates.map((c) => c.id)).not.toContain(inbox.issueId);
      });

      await test.step("picking a target marks the request duplicate with a link", async () => {
        await driver.intakeTriageDuplicateSearch(targetName);
        await driver.intakeTriageDuplicatePick(targetName);
        const link = await driver.intakeTriageDuplicateOfText();
        expect(link).toContain(`${identifier}-${target.sequence_id}`);
        const stored = await serverIntakeTriageDetail(seed.workspaceSlug, projectId, inbox.issueId, session);
        expect(stored.status).toBe(STATUS_DUPLICATE);
        expect(stored.duplicateTo).toBe(targetId);
        expect(await driver.intakeTriageCurrentIssueId()).toBe(inbox.issueId);
        expect(await driver.intakeTriageAcceptVisible()).toBe(false);
      });
    } finally {
      await serverCleanupInboxIssue(seed.workspaceSlug, projectId, inbox.issueId, session);
      await serverDeleteIssue(seed.workspaceSlug, projectId, targetId, session).catch(() => undefined);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["INT-025"], "delete removes a pending request and its work item, then returns to the list"),
  { tag: specTags(["INT-025"]) },
  async ({ driver, seed }) => {
    test.setTimeout(420_000);
    const tag = `NF259 delete ${Date.now()}`;
    const session = await signInSessionRetry(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N259"),
      { inboxView: true },
      session
    );
    const first = await serverCreateInboxIssue(seed.workspaceSlug, projectId, `${tag} A`, session);
    const second = await serverCreateInboxIssue(seed.workspaceSlug, projectId, `${tag} B`, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.rulesOpenIntakeIssue(seed.workspaceSlug, projectId, first.issueId);
      await expect.poll(() => driver.rulesIntakeTriageVisible(), { timeout: 60_000 }).toBe(true);

      await test.step("the delete dialog confirms an irreversible removal", async () => {
        const options = await driver.intakeTriageOverflowOptions();
        expect(options).toContain("Delete");
        await driver.intakeTriageClickDelete();
        expect(await driver.intakeTriageDeleteDialogVisible()).toBe(true);
        const text = await driver.intakeTriageDeleteDialogText();
        expect(text.heading).toBe("Delete work item");
        expect(text.body).toContain("cannot be undone");
      });

      await test.step("cancelling keeps the request", async () => {
        await driver.intakeTriageDeleteCancel();
        expect(await driver.intakeTriageDeleteDialogVisible()).toBe(false);
        expect(await driver.intakeTriageCurrentIssueId()).toBe(first.issueId);
        const stored = await serverIntakeTriageDetail(seed.workspaceSlug, projectId, first.issueId, session);
        expect(stored.status).toBe(STATUS_PENDING);
      });

      await test.step("confirming removes both rows and advances to the neighbour", async () => {
        await driver.intakeTriageClickDelete();
        await driver.intakeTriageDeleteConfirm();
        // Secondary INT-028 assertion: the URL tracks the move (owned by NEWFRONT-258).
        await expect.poll(() => driver.intakeTriageCurrentIssueId(), { timeout: 30_000 }).toBe(second.issueId);
        const remaining = await serverInboxIssues(seed.workspaceSlug, projectId, session);
        expect(remaining.map((r) => r.issueId)).not.toContain(first.issueId);
        expect(await serverIntakeTriageWorkItemExists(seed.workspaceSlug, projectId, first.issueId, session)).toBe(
          false
        );
      });
    } finally {
      await serverCleanupInboxIssue(seed.workspaceSlug, projectId, first.issueId, session);
      await serverCleanupInboxIssue(seed.workspaceSlug, projectId, second.issueId, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["INT-025"], "delete keeps an accepted work item but cascades for every other status"),
  { tag: specTags(["INT-025"]) },
  async ({ driver, seed }) => {
    test.setTimeout(420_000);
    const tag = `NF259 cascade ${Date.now()}`;
    const session = await signInSessionRetry(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N259"),
      { inboxView: true },
      session
    );
    const accepted = await serverCreateInboxIssue(seed.workspaceSlug, projectId, `${tag} accepted`, session);
    const declined = await serverCreateInboxIssue(seed.workspaceSlug, projectId, `${tag} declined`, session);
    const duplicate = await serverCreateInboxIssue(seed.workspaceSlug, projectId, `${tag} duplicate`, session);
    const pending = await serverCreateInboxIssue(seed.workspaceSlug, projectId, `${tag} pending`, session);
    const targetId = await serverCreateIssue(seed.workspaceSlug, projectId, session, `${tag} target`);
    await serverIntakeTriagePatch(
      seed.workspaceSlug,
      projectId,
      accepted.issueId,
      { status: STATUS_ACCEPTED },
      session
    );
    await serverIntakeTriagePatch(
      seed.workspaceSlug,
      projectId,
      declined.issueId,
      { status: STATUS_DECLINED },
      session
    );
    await serverIntakeTriagePatch(
      seed.workspaceSlug,
      projectId,
      duplicate.issueId,
      { status: STATUS_DUPLICATE, duplicate_to: targetId },
      session
    );
    try {
      await test.step("deleting an accepted request keeps its work item", async () => {
        expect(await serverIntakeTriageDeleteRaw(seed.workspaceSlug, projectId, accepted.issueId, session)).toBe(204);
        const remaining = await serverInboxIssues(seed.workspaceSlug, projectId, session);
        expect(remaining.map((r) => r.issueId)).not.toContain(accepted.issueId);
        expect(await serverIntakeTriageWorkItemExists(seed.workspaceSlug, projectId, accepted.issueId, session)).toBe(
          true
        );
      });

      await test.step("deleting declined, duplicate and pending requests cascades", async () => {
        for (const row of [declined, duplicate, pending]) {
          expect(await serverIntakeTriageDeleteRaw(seed.workspaceSlug, projectId, row.issueId, session)).toBe(204);
          expect(await serverIntakeTriageWorkItemExists(seed.workspaceSlug, projectId, row.issueId, session)).toBe(
            false
          );
        }
        const remaining = await serverInboxIssues(seed.workspaceSlug, projectId, session);
        expect(remaining.map((r) => r.issueId)).toEqual([]);
      });

      await test.step("resolved requests offer no delete control in the UI", async () => {
        const probe = await serverCreateInboxIssue(seed.workspaceSlug, projectId, `${tag} probe`, session);
        try {
          await serverIntakeTriagePatch(
            seed.workspaceSlug,
            projectId,
            probe.issueId,
            { status: STATUS_DECLINED },
            session
          );
          await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
          await driver.intakeTriageOpenIssueOnTab(seed.workspaceSlug, projectId, probe.issueId, "closed");
          await expect.poll(() => driver.intakeTriageStatusChipText(), { timeout: 60_000 }).toBe("Declined");
          expect(await driver.intakeTriageOverflowVisible()).toBe(false);
        } finally {
          await serverCleanupInboxIssue(seed.workspaceSlug, projectId, probe.issueId, session);
        }
      });
    } finally {
      await serverDeleteIssue(seed.workspaceSlug, projectId, accepted.issueId, session).catch(() => undefined);
      await serverDeleteIssue(seed.workspaceSlug, projectId, targetId, session).catch(() => undefined);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["INT-025"], "delete is refused for outsiders but allowed for the request creator"),
  { tag: specTags(["INT-025"]) },
  async ({ driver, seed }) => {
    test.setTimeout(420_000);
    const member = seed.mentionMember;
    if (!member) throw new Error("[parity] seed has no member identity; rerun parity-up.sh from a clean checkout.");
    const tag = `NF259 delperm ${Date.now()}`;
    const admin = await signInSessionRetry(seed.email, seed.password);
    const memberSession = await signInSessionRetry(member.email, member.password);
    const foreign = await serverCreateInboxIssue(seed.workspaceSlug, seed.projectId, `${tag} foreign`, admin);
    const own = await serverCreateInboxIssue(seed.workspaceSlug, seed.projectId, `${tag} own`, memberSession);
    try {
      await test.step("a member cannot delete or retriage someone else's request", async () => {
        expect(
          await serverIntakeTriageDeleteRaw(seed.workspaceSlug, seed.projectId, foreign.issueId, memberSession)
        ).toBe(403);
        const refused = await serverIntakeTriagePatchRaw(
          seed.workspaceSlug,
          seed.projectId,
          foreign.issueId,
          {
            status: STATUS_ACCEPTED,
          },
          memberSession
        );
        expect(refused.status).toBe(403);
        const stored = await serverIntakeTriageDetail(seed.workspaceSlug, seed.projectId, foreign.issueId, admin);
        expect(stored.status).toBe(STATUS_PENDING);
      });

      await test.step("the UI offers no delete control on someone else's request", async () => {
        await driver.rulesEnsureSignedIn(member.email, member.password, seed.workspaceSlug);
        await driver.rulesOpenIntakeIssue(seed.workspaceSlug, seed.projectId, foreign.issueId);
        await expect.poll(() => driver.rulesIntakeTriageVisible(), { timeout: 60_000 }).toBe(true);
        const options = await driver.intakeTriageOverflowOptions();
        expect(options).not.toContain("Delete");
      });

      await test.step("the creator deletes their own request through the dialog", async () => {
        await driver.rulesOpenIntakeIssue(seed.workspaceSlug, seed.projectId, own.issueId);
        await expect.poll(() => driver.rulesIntakeTriageVisible(), { timeout: 60_000 }).toBe(true);
        const options = await driver.intakeTriageOverflowOptions();
        expect(options).toContain("Delete");
        await driver.intakeTriageClickDelete();
        await driver.intakeTriageDeleteConfirm();
        await expect.poll(() => driver.intakeTriageCurrentIssueId(), { timeout: 60_000 }).not.toBe(own.issueId);
        const remaining = await serverInboxIssues(seed.workspaceSlug, seed.projectId, admin);
        expect(remaining.map((r) => r.issueId)).not.toContain(own.issueId);
        expect(await serverIntakeTriageWorkItemExists(seed.workspaceSlug, seed.projectId, own.issueId, admin)).toBe(
          false
        );
      });
    } finally {
      await serverCleanupInboxIssue(seed.workspaceSlug, seed.projectId, foreign.issueId, admin);
      await serverCleanupInboxIssue(seed.workspaceSlug, seed.projectId, own.issueId, admin);
    }
  }
);
