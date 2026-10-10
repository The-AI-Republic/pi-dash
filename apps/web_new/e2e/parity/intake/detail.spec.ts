// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-258): intake request detail — the detail
// pane (title, description, reactions, attachments, properties),
// copy/open work-item link, edit permissions, keyboard and chevron
// navigation, deep-linking, and description versions.
// Rows: INT-019, INT-024, INT-026, INT-027, INT-028, INT-029.
//
// Observed behavior notes (the inventory rows carry them at update
// time): the title is an inline field while editable and a static
// block once read-only; title and description autosave and report
// through a Saved indicator; the state row is always disabled while
// assignees, priority, due date and labels follow editability (labels
// omit their add control when disabled); copy lives behind a direct
// button on resolved items and an admin/member-only overflow menu on
// open ones; only admins and creators edit, guests who own an item see
// a fully editable UI while the server keeps only their name and
// description writes, and resolved items are UI read-only; arrows and
// chevrons cycle with wrap and stay quiet while typing; deep-links
// carry currentTab plus inboxIssueId and stale ids fall back to the
// list; description versions start with one row at creation and a
// second author (same-user edits fold into one row for ten minutes)
// grows the list, and restoring swaps the current description. Byte
// uploads need stack object storage, which is stubbed on this machine
// (see NEWFRONT-143), so attachments are proven through API-created
// rows that list and render.
import { test, expect } from "../fixtures";
import {
  issueAttachments,
  parityProjectIdentifier,
  serverAddProjectMembers,
  serverCleanupInboxIssue,
  serverCleanupProject,
  serverCreateInboxIssue,
  serverCreateLabel,
  serverCreateProjectWithFlags,
  serverIntakeDetailCreateAttachment,
  serverIntakeDetailPatch,
  serverIntakeDetailRead,
  serverIntakeDetailSetStatus,
  serverIntakeDetailVersion,
  serverIntakeDetailVersions,
  serverIssueReactions,
  serverPatchProject,
  serverProjectMembers,
  signInSessionRetry,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";

/** Seed guest credentials, or a thrown error naming the missing seed step. */
function requireGuest(seed: ParitySeedFacts): { email: string; password: string } {
  if (!seed.guestEmail || !seed.guestPassword)
    throw new Error("[parity] seed has no guest identity; rerun parity-up.sh from a clean checkout.");
  return { email: seed.guestEmail, password: seed.guestPassword };
}

/** Seed guest user id, resolved live (never hardcoded across stacks). */
async function seedGuestUserId(seed: ParitySeedFacts, session: string): Promise<string> {
  const members = await serverProjectMembers(seed.workspaceSlug, seed.projectId, session);
  const guest = members.find((row) => row.role === 5);
  if (!guest) throw new Error("[parity] seed project has no role-5 guest member.");
  return guest.userId;
}

/** Best-effort cleanup of scenario-owned issues plus their project. */
async function dropProject(
  seed: ParitySeedFacts,
  projectId: string,
  issueIds: string[],
  session: string
): Promise<void> {
  for (const issueId of issueIds) {
    await serverCleanupInboxIssue(seed.workspaceSlug, projectId, issueId, session);
  }
  await serverCleanupProject(seed.workspaceSlug, projectId, session);
}

/**
 * Open a tab item past the tab-store race (list first, id after),
 * reopening twice: the oracle dev server stalls whole renders under
 * shared-box load, and a stalled first open must not fail the scenario.
 * A genuinely missing row still fails on the final strict pass.
 */
async function openTabItem(
  driver: ParityDriver,
  seed: ParitySeedFacts,
  projectId: string,
  tab: "open" | "closed",
  issueId: string,
  title: string
): Promise<void> {
  const base = `/${seed.workspaceSlug}/projects/${projectId}/intake?currentTab=${tab}`;
  for (let round = 1; round <= 2; round++) {
    await driver.intakeDetailOpenRaw(base);
    const listed = await expect
      .poll(() => driver.intakeDetailListVisible(), { timeout: 60_000 })
      .toBe(true)
      .then(
        () => true,
        () => false
      );
    if (!listed) continue;
    await driver.intakeDetailOpenRaw(`${base}&inboxIssueId=${issueId}`);
    const opened = await expect
      .poll(() => driver.intakeDetailTitle(), { timeout: 60_000 })
      .toBe(title)
      .then(
        () => true,
        () => false
      );
    if (opened) return;
  }
  await driver.intakeDetailOpenRaw(base);
  await expect.poll(() => driver.intakeDetailListVisible(), { timeout: 90_000 }).toBe(true);
  await driver.intakeDetailOpenRaw(`${base}&inboxIssueId=${issueId}`);
  await expect.poll(() => driver.intakeDetailTitle(), { timeout: 90_000 }).toBe(title);
}

/** Open a closed-tab item past the tab-store race (list first, id after). */
async function openClosedItem(
  driver: ParityDriver,
  seed: ParitySeedFacts,
  projectId: string,
  issueId: string,
  title: string
): Promise<void> {
  await openTabItem(driver, seed, projectId, "closed", issueId, title);
}

/** Open an open-tab item past the tab-store race (list first, id after). */
async function openOpenItem(
  driver: ParityDriver,
  seed: ParitySeedFacts,
  projectId: string,
  issueId: string,
  title: string
): Promise<void> {
  await openTabItem(driver, seed, projectId, "open", issueId, title);
}

test(
  specTitle(["INT-019"], "admin edits the title and description inline and both persist"),
  { tag: specTags(["INT-019"]) },
  async ({ driver, seed }) => {
    const tag = `NF258 title ${Date.now()}`;
    const session = await signInSessionRetry(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N258"),
      { inboxView: true },
      session
    );
    const name = `${tag} request`;
    const inbox = await serverCreateInboxIssue(seed.workspaceSlug, projectId, name, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await openOpenItem(driver, seed, projectId, inbox.issueId, name);

      await test.step("the detail shows the title and description as editable", async () => {
        await expect.poll(() => driver.intakeDetailTitleEditable(), { timeout: 30_000 }).toBe(true);
        await expect.poll(() => driver.intakeDetailDescriptionEditable(), { timeout: 30_000 }).toBe(true);
      });

      const renamed = `${name} renamed`;
      await test.step("a title edit saves and reads back on the server", async () => {
        await driver.intakeDetailSetTitle(renamed);
        await expect
          .poll(
            async () => (await serverIntakeDetailRead(seed.workspaceSlug, projectId, inbox.issueId, session)).name,
            {
              timeout: 30_000,
            }
          )
          .toBe(renamed);
        expect(await driver.intakeDetailTitle()).toBe(renamed);
      });

      const body = `detail body ${Date.now()}`;
      await test.step("a description edit saves and reads back on the server", async () => {
        await driver.intakeDetailSetDescription(body);
        await expect
          .poll(
            async () =>
              (await serverIntakeDetailRead(seed.workspaceSlug, projectId, inbox.issueId, session)).descriptionHtml,
            { timeout: 30_000 }
          )
          .toContain(body);
        expect(await driver.intakeDetailDescriptionText()).toContain(body);
      });
    } finally {
      await dropProject(seed, projectId, [inbox.issueId], session);
    }
  }
);

test(
  specTitle(["INT-019"], "state reads disabled while assignees, priority, due date and labels edit and persist"),
  { tag: specTags(["INT-019"]) },
  async ({ driver, seed }) => {
    const tag = `NF258 props ${Date.now()}`;
    const session = await signInSessionRetry(seed.email, seed.password);
    const member = seed.mentionMember;
    if (!member) throw new Error("[parity] seed has no mentionMember; rerun parity-up.sh from a clean checkout.");
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N258"),
      { inboxView: true },
      session
    );
    await serverAddProjectMembers(seed.workspaceSlug, projectId, [{ memberId: member.id, role: 15 }], session);
    const label = await serverCreateLabel(seed.workspaceSlug, projectId, `${tag} lab`, "#336699", session);
    const name = `${tag} request`;
    const inbox = await serverCreateInboxIssue(seed.workspaceSlug, projectId, name, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await openOpenItem(driver, seed, projectId, inbox.issueId, name);

      await test.step("state is disabled and the rest enable for an admin", async () => {
        await expect.poll(() => driver.intakeDetailPropertyDisabled("State"), { timeout: 30_000 }).toBe(true);
        await expect.poll(() => driver.intakeTriageStateDisabled(), { timeout: 30_000 }).toBe(true);
        await expect.poll(() => driver.intakeDetailPropertyDisabled("Assignees"), { timeout: 30_000 }).toBe(false);
        await expect.poll(() => driver.intakeDetailPropertyDisabled("Priority"), { timeout: 30_000 }).toBe(false);
        await expect.poll(() => driver.intakeDetailPropertyDisabled("Due date"), { timeout: 30_000 }).toBe(false);
        await expect.poll(() => driver.intakeDetailPropertyDisabled("Labels"), { timeout: 30_000 }).toBe(false);
      });

      await test.step("a priority pick persists", async () => {
        await driver.intakeDetailSetPriority("High");
        await expect
          .poll(
            async () => (await serverIntakeDetailRead(seed.workspaceSlug, projectId, inbox.issueId, session)).priority,
            { timeout: 30_000 }
          )
          .toBe("high");
        expect(await driver.intakeDetailPriorityText()).toContain("High");
      });

      await test.step("adding an assignee persists", async () => {
        await driver.intakeDetailAddAssignee(member.displayName);
        await expect
          .poll(
            async () =>
              (await serverIntakeDetailRead(seed.workspaceSlug, projectId, inbox.issueId, session)).assigneeIds,
            { timeout: 30_000 }
          )
          .toContain(member.id);
        const shown = await driver.intakeDetailAssigneeTexts();
        expect(shown.join(" ")).not.toBe("Add assignees");
      });

      await test.step("a due-date pick persists", async () => {
        await driver.intakeDetailSetDueDate(7);
        const target = new Date();
        target.setDate(target.getDate() + 7);
        const expected = `${target.getFullYear()}-${String(target.getMonth() + 1).padStart(2, "0")}-${String(
          target.getDate()
        ).padStart(2, "0")}`;
        await expect
          .poll(
            async () =>
              (await serverIntakeDetailRead(seed.workspaceSlug, projectId, inbox.issueId, session)).targetDate,
            { timeout: 30_000 }
          )
          .toBe(expected);
        expect(await driver.intakeDetailDueDateText()).not.toBe("Add due date");
      });

      await test.step("attaching a label persists", async () => {
        await driver.intakeDetailAddLabel(label.name);
        await expect
          .poll(
            async () => (await serverIntakeDetailRead(seed.workspaceSlug, projectId, inbox.issueId, session)).labelIds,
            { timeout: 30_000 }
          )
          .toContain(label.id);
        await expect.poll(() => driver.intakeDetailLabelTexts(), { timeout: 30_000 }).toContain(label.name);
      });
    } finally {
      await dropProject(seed, projectId, [inbox.issueId], session);
    }
  }
);

test(
  specTitle(["INT-019"], "reactions add and attachments list on the detail"),
  { tag: specTags(["INT-019"]) },
  async ({ driver, seed }) => {
    const tag = `NF258 react ${Date.now()}`;
    const session = await signInSessionRetry(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N258"),
      { inboxView: true },
      session
    );
    const name = `${tag} request`;
    const inbox = await serverCreateInboxIssue(seed.workspaceSlug, projectId, name, session);
    // Short stem: the list truncates long file names with an ellipsis.
    const fileStem = `nf${Date.now().toString(36)}`;
    await serverIntakeDetailCreateAttachment(
      seed.workspaceSlug,
      projectId,
      inbox.issueId,
      `${fileStem}.png`,
      "image/png",
      68,
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await openOpenItem(driver, seed, projectId, inbox.issueId, name);

      await test.step("the API-created attachment lists in the UI and on the server", async () => {
        await expect.poll(() => driver.intakeDetailAttachmentNames(), { timeout: 60_000 }).toContain(fileStem);
        const rows = await issueAttachments(seed.workspaceSlug, projectId, inbox.issueId, session);
        const names = rows.map((row) => {
          if (typeof row["name"] === "string") return row["name"] as string;
          const attrs = row["attributes"] as Record<string, unknown> | undefined;
          return typeof attrs?.["name"] === "string" ? (attrs["name"] as string) : "";
        });
        expect(names).toContain(`${fileStem}.png`);
      });

      await test.step("adding a reaction shows its chip and lands on the server", async () => {
        expect(await driver.intakeDetailReactions()).toEqual([]);
        await driver.intakeDetailAddReaction("😀");
        // The server stores the reaction as the decimal codepoint (grinning
        // face is U+1F600) while the UI renders the glyph.
        await expect
          .poll(async () => serverIssueReactions(seed.workspaceSlug, projectId, inbox.issueId, session), {
            timeout: 30_000,
          })
          .toEqual(expect.arrayContaining([expect.objectContaining({ reaction: "128512" })]));
        const chips = await driver.intakeDetailReactions();
        expect(chips.some((chip) => chip.includes("😀"))).toBe(true);
      });
    } finally {
      await dropProject(seed, projectId, [inbox.issueId], session);
    }
  }
);

/** Work-item link shape the copy action must place on the clipboard. */
function workItemLinkPattern(seed: ParitySeedFacts, identifier: string): RegExp {
  const oracleBase = (process.env["PARITY_ORACLE_URL"] ?? "http://localhost:13058").replace(/\/+$/, "");
  const escapedBase = oracleBase.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const escapedSlug = seed.workspaceSlug.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return new RegExp(`^${escapedBase}/${escapedSlug}/browse/${identifier}-\\d+/$`);
}

test(
  specTitle(["INT-024"], "copy places the work-item link on the clipboard and resolved items open it"),
  { tag: specTags(["INT-024"]) },
  async ({ driver, seed }) => {
    const tag = `NF258 copy ${Date.now()}`;
    const session = await signInSessionRetry(seed.email, seed.password);
    const identifier = parityProjectIdentifier("N258");
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      identifier,
      {
        inboxView: true,
      },
      session
    );
    const openName = `${tag} open request`;
    const open = await serverCreateInboxIssue(seed.workspaceSlug, projectId, openName, session);
    const resolvedName = `${tag} resolved request`;
    const resolved = await serverCreateInboxIssue(seed.workspaceSlug, projectId, resolvedName, session);
    await serverIntakeDetailSetStatus(seed.workspaceSlug, projectId, resolved.issueId, 1, session);
    const linkPattern = workItemLinkPattern(seed, identifier);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("an open item copies its link from the overflow menu with a toast", async () => {
        await openOpenItem(driver, seed, projectId, open.issueId, openName);
        await expect.poll(() => driver.intakeDetailOpenWorkItemVisible(), { timeout: 30_000 }).toBe(false);
        await driver.intakeDetailCopyLink();
        let toast: { title: string; message: string } | null = null;
        await expect.poll(async () => (toast = await driver.rulesLastToast()), { timeout: 15_000 }).not.toBeNull();
        expect(`${toast?.title ?? ""} ${toast?.message ?? ""}`.toLowerCase()).toContain("copied");
        expect(await driver.rulesReadClipboard()).toMatch(linkPattern);
      });

      let resolvedLink = "";
      await test.step("a resolved item copies from its button and opens the work item", async () => {
        await openClosedItem(driver, seed, projectId, resolved.issueId, resolvedName);
        await expect.poll(() => driver.intakeDetailOpenWorkItemVisible(), { timeout: 30_000 }).toBe(true);
        await driver.intakeDetailCopyLink();
        await expect.poll(() => driver.rulesLastToast(), { timeout: 15_000 }).not.toBeNull();
        resolvedLink = await driver.rulesReadClipboard();
        expect(resolvedLink).toMatch(linkPattern);
        await driver.intakeDetailOpenWorkItem();
        expect(await driver.intakeDetailCurrentUrl()).toBe(resolvedLink);
      });
    } finally {
      await dropProject(seed, projectId, [open.issueId, resolved.issueId], session);
    }
  }
);

test(
  specTitle(["INT-024"], "a member copies an open item from the overflow menu"),
  { tag: specTags(["INT-024"]) },
  async ({ driver, seed }) => {
    // Own test (own browser context): re-signing in over an active
    // session is a silent no-op, so each actor gets a fresh context.
    const tag = `NF258 mcopy ${Date.now()}`;
    const session = await signInSessionRetry(seed.email, seed.password);
    const member = seed.mentionMember;
    if (!member) throw new Error("[parity] seed has no mentionMember; rerun parity-up.sh from a clean checkout.");
    const identifier = parityProjectIdentifier("N258");
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      identifier,
      {
        inboxView: true,
      },
      session
    );
    await serverAddProjectMembers(seed.workspaceSlug, projectId, [{ memberId: member.id, role: 15 }], session);
    const openName = `${tag} open request`;
    const open = await serverCreateInboxIssue(seed.workspaceSlug, projectId, openName, session);
    try {
      await driver.rulesEnsureSignedIn(member.email, member.password, seed.workspaceSlug);
      await openOpenItem(driver, seed, projectId, open.issueId, openName);
      await driver.intakeDetailCopyLink();
      expect(await driver.rulesReadClipboard()).toMatch(workItemLinkPattern(seed, identifier));
    } finally {
      await dropProject(seed, projectId, [open.issueId], session);
    }
  }
);

test(
  specTitle(["INT-026"], "an admin edits everything while the server refuses member writes on others"),
  { tag: specTags(["INT-026"]) },
  async ({ driver, seed }) => {
    const tag = `NF258 permits ${Date.now()}`;
    const session = await signInSessionRetry(seed.email, seed.password);
    const member = seed.mentionMember;
    if (!member) throw new Error("[parity] seed has no mentionMember; rerun parity-up.sh from a clean checkout.");
    const memberSession = await signInSessionRetry(member.email, member.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N258"),
      { inboxView: true },
      session
    );
    await serverAddProjectMembers(seed.workspaceSlug, projectId, [{ memberId: member.id, role: 15 }], session);
    const ownerName = `${tag} owner request`;
    const owner = await serverCreateInboxIssue(seed.workspaceSlug, projectId, ownerName, session);
    const memberName = `${tag} member request`;
    const memberInbox = await serverCreateInboxIssue(seed.workspaceSlug, projectId, memberName, memberSession);
    try {
      await test.step("an admin edits everything and sees versions", async () => {
        await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
        await openOpenItem(driver, seed, projectId, owner.issueId, ownerName);
        await expect.poll(() => driver.intakeDetailTitleEditable(), { timeout: 30_000 }).toBe(true);
        await expect.poll(() => driver.intakeDetailDescriptionEditable(), { timeout: 30_000 }).toBe(true);
        await expect.poll(() => driver.intakeDetailPropertyDisabled("State"), { timeout: 30_000 }).toBe(true);
        await expect.poll(() => driver.intakeDetailPropertyDisabled("Assignees"), { timeout: 30_000 }).toBe(false);
        await expect.poll(() => driver.intakeDetailPropertyDisabled("Priority"), { timeout: 30_000 }).toBe(false);
        await expect.poll(() => driver.intakeDetailPropertyDisabled("Due date"), { timeout: 30_000 }).toBe(false);
        await expect.poll(() => driver.intakeDetailPropertyDisabled("Labels"), { timeout: 30_000 }).toBe(false);
        await expect.poll(() => driver.intakeDetailVersionsVisible(), { timeout: 30_000 }).toBe(true);
      });

      await test.step("the server refuses the member write and allows the creator write", async () => {
        const refused = await serverIntakeDetailPatch(
          seed.workspaceSlug,
          projectId,
          owner.issueId,
          { issue: { name: `${ownerName} hacked` } },
          memberSession
        );
        expect(refused.status).toBe(403);
        const allowed = await serverIntakeDetailPatch(
          seed.workspaceSlug,
          projectId,
          memberInbox.issueId,
          { issue: { description_html: "<p>member own edit</p>" } },
          memberSession
        );
        expect(allowed.status).toBe(200);
        const read = await serverIntakeDetailRead(seed.workspaceSlug, projectId, owner.issueId, session);
        expect(read.name).toBe(ownerName);
      });
    } finally {
      await dropProject(seed, projectId, [owner.issueId, memberInbox.issueId], session);
    }
  }
);

test(
  specTitle(["INT-026"], "a member edits their own request and reads others read-only"),
  { tag: specTags(["INT-026"]) },
  async ({ driver, seed }) => {
    // Own test (own browser context): re-signing in over an active
    // session is a silent no-op, so each actor gets a fresh context.
    const tag = `NF258 member ${Date.now()}`;
    const session = await signInSessionRetry(seed.email, seed.password);
    const member = seed.mentionMember;
    if (!member) throw new Error("[parity] seed has no mentionMember; rerun parity-up.sh from a clean checkout.");
    const memberSession = await signInSessionRetry(member.email, member.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N258"),
      { inboxView: true },
      session
    );
    await serverAddProjectMembers(seed.workspaceSlug, projectId, [{ memberId: member.id, role: 15 }], session);
    const ownerName = `${tag} owner request`;
    const owner = await serverCreateInboxIssue(seed.workspaceSlug, projectId, ownerName, session);
    const memberName = `${tag} member request`;
    const memberInbox = await serverCreateInboxIssue(seed.workspaceSlug, projectId, memberName, memberSession);
    try {
      await driver.rulesEnsureSignedIn(member.email, member.password, seed.workspaceSlug);

      await test.step("a member edits their own request", async () => {
        await openOpenItem(driver, seed, projectId, memberInbox.issueId, memberName);
        await expect.poll(() => driver.intakeDetailTitleEditable(), { timeout: 30_000 }).toBe(true);
        await expect.poll(() => driver.intakeDetailDescriptionEditable(), { timeout: 30_000 }).toBe(true);
        await expect.poll(() => driver.intakeDetailPropertyDisabled("Assignees"), { timeout: 30_000 }).toBe(false);
        await expect.poll(() => driver.intakeDetailPropertyDisabled("Priority"), { timeout: 30_000 }).toBe(false);
        await expect.poll(() => driver.intakeDetailPropertyDisabled("Due date"), { timeout: 30_000 }).toBe(false);
        await expect.poll(() => driver.intakeDetailPropertyDisabled("Labels"), { timeout: 30_000 }).toBe(false);
        await expect.poll(() => driver.intakeDetailVersionsVisible(), { timeout: 30_000 }).toBe(true);
      });

      await test.step("a member reads another request read-only", async () => {
        await openOpenItem(driver, seed, projectId, owner.issueId, ownerName);
        await expect.poll(() => driver.intakeDetailTitleEditable(), { timeout: 30_000 }).toBe(false);
        await expect.poll(() => driver.intakeDetailDescriptionEditable(), { timeout: 30_000 }).toBe(false);
        await expect.poll(() => driver.intakeDetailPropertyDisabled("State"), { timeout: 30_000 }).toBe(true);
        await expect.poll(() => driver.intakeDetailPropertyDisabled("Assignees"), { timeout: 30_000 }).toBe(true);
        await expect.poll(() => driver.intakeDetailPropertyDisabled("Priority"), { timeout: 30_000 }).toBe(true);
        await expect.poll(() => driver.intakeDetailPropertyDisabled("Due date"), { timeout: 30_000 }).toBe(true);
        await expect.poll(() => driver.intakeDetailPropertyDisabled("Labels"), { timeout: 30_000 }).toBe(true);
        await expect.poll(() => driver.intakeDetailVersionsVisible(), { timeout: 30_000 }).toBe(false);
      });
    } finally {
      await dropProject(seed, projectId, [owner.issueId, memberInbox.issueId], session);
    }
  }
);

test(
  specTitle(["INT-026"], "guests read others read-only and write only title and description on their own"),
  { tag: specTags(["INT-026"]) },
  async ({ driver, seed }) => {
    const tag = `NF258 guest ${Date.now()}`;
    const session = await signInSessionRetry(seed.email, seed.password);
    const guestCreds = requireGuest(seed);
    const guestSession = await signInSessionRetry(guestCreds.email, guestCreds.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N258"),
      { inboxView: true },
      session
    );
    await serverPatchProject(seed.workspaceSlug, projectId, session, { guest_view_all_features: true });
    const guestId = await seedGuestUserId(seed, session);
    await serverAddProjectMembers(seed.workspaceSlug, projectId, [{ memberId: guestId, role: 5 }], session);
    const ownerName = `${tag} owner request`;
    const owner = await serverCreateInboxIssue(seed.workspaceSlug, projectId, ownerName, session);
    const guestName = `${tag} guest request`;
    const guestInbox = await serverCreateInboxIssue(seed.workspaceSlug, projectId, guestName, guestSession);
    try {
      await driver.rulesEnsureSignedIn(guestCreds.email, guestCreds.password, seed.workspaceSlug);

      await test.step("a guest reads another request read-only", async () => {
        await openOpenItem(driver, seed, projectId, owner.issueId, ownerName);
        await expect.poll(() => driver.intakeDetailTitleEditable(), { timeout: 30_000 }).toBe(false);
        await expect.poll(() => driver.intakeDetailDescriptionEditable(), { timeout: 30_000 }).toBe(false);
        await expect.poll(() => driver.intakeDetailPropertyDisabled("State"), { timeout: 30_000 }).toBe(true);
        await expect.poll(() => driver.intakeDetailPropertyDisabled("Assignees"), { timeout: 30_000 }).toBe(true);
        await expect.poll(() => driver.intakeDetailPropertyDisabled("Priority"), { timeout: 30_000 }).toBe(true);
        await expect.poll(() => driver.intakeDetailPropertyDisabled("Due date"), { timeout: 30_000 }).toBe(true);
        await expect.poll(() => driver.intakeDetailPropertyDisabled("Labels"), { timeout: 30_000 }).toBe(true);
        await expect.poll(() => driver.intakeDetailVersionsVisible(), { timeout: 30_000 }).toBe(false);
      });

      await test.step("a guest sees their own request editable", async () => {
        await openOpenItem(driver, seed, projectId, guestInbox.issueId, guestName);
        await expect.poll(() => driver.intakeDetailTitleEditable(), { timeout: 30_000 }).toBe(true);
        await expect.poll(() => driver.intakeDetailDescriptionEditable(), { timeout: 30_000 }).toBe(true);
        // The UI offers the full panel to an owning guest; the server
        // narrows the writes (asserted below), it does not gate the UI.
        await expect.poll(() => driver.intakeDetailPropertyDisabled("Assignees"), { timeout: 30_000 }).toBe(false);
        await expect.poll(() => driver.intakeDetailPropertyDisabled("Priority"), { timeout: 30_000 }).toBe(false);
        await expect.poll(() => driver.intakeDetailPropertyDisabled("Due date"), { timeout: 30_000 }).toBe(false);
        await expect.poll(() => driver.intakeDetailPropertyDisabled("Labels"), { timeout: 30_000 }).toBe(false);
        await expect.poll(() => driver.intakeDetailVersionsVisible(), { timeout: 30_000 }).toBe(true);
      });

      await test.step("the server refuses other writes and narrows own writes to title and description", async () => {
        const refused = await serverIntakeDetailPatch(
          seed.workspaceSlug,
          projectId,
          owner.issueId,
          { issue: { name: `${ownerName} hacked` } },
          guestSession
        );
        expect(refused.status).toBe(403);
        const renamed = `${guestName} renamed`;
        const nameWrite = await serverIntakeDetailPatch(
          seed.workspaceSlug,
          projectId,
          guestInbox.issueId,
          { issue: { name: renamed } },
          guestSession
        );
        expect(nameWrite.status).toBe(200);
        const priorityWrite = await serverIntakeDetailPatch(
          seed.workspaceSlug,
          projectId,
          guestInbox.issueId,
          { issue: { priority: "urgent" } },
          guestSession
        );
        expect(priorityWrite.status).toBe(200);
        const read = await serverIntakeDetailRead(seed.workspaceSlug, projectId, guestInbox.issueId, session);
        expect(read.name).toBe(renamed);
        expect(read.priority).not.toBe("urgent");
        const ownerRead = await serverIntakeDetailRead(seed.workspaceSlug, projectId, owner.issueId, session);
        expect(ownerRead.name).toBe(ownerName);
      });
    } finally {
      await dropProject(seed, projectId, [owner.issueId, guestInbox.issueId], session);
    }
  }
);

test(
  specTitle(["INT-026"], "resolved items read read-only for every viewer"),
  { tag: specTags(["INT-026"]) },
  async ({ driver, seed }) => {
    const tag = `NF258 resolved ${Date.now()}`;
    const session = await signInSessionRetry(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N258"),
      { inboxView: true },
      session
    );
    const name = `${tag} request`;
    const inbox = await serverCreateInboxIssue(seed.workspaceSlug, projectId, name, session);
    await serverIntakeDetailSetStatus(seed.workspaceSlug, projectId, inbox.issueId, 1, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await openClosedItem(driver, seed, projectId, inbox.issueId, name);

      await expect.poll(() => driver.intakeDetailTitleEditable(), { timeout: 30_000 }).toBe(false);
      await expect.poll(() => driver.intakeDetailDescriptionEditable(), { timeout: 30_000 }).toBe(false);
      await expect.poll(() => driver.intakeDetailPropertyDisabled("State"), { timeout: 30_000 }).toBe(true);
      await expect.poll(() => driver.intakeDetailPropertyDisabled("Assignees"), { timeout: 30_000 }).toBe(true);
      await expect.poll(() => driver.intakeDetailPropertyDisabled("Priority"), { timeout: 30_000 }).toBe(true);
      await expect.poll(() => driver.intakeDetailPropertyDisabled("Due date"), { timeout: 30_000 }).toBe(true);
      await expect.poll(() => driver.intakeDetailPropertyDisabled("Labels"), { timeout: 30_000 }).toBe(true);
      await expect.poll(() => driver.intakeDetailVersionsVisible(), { timeout: 30_000 }).toBe(false);

      const read = await serverIntakeDetailRead(seed.workspaceSlug, projectId, inbox.issueId, session);
      expect(read.status).toBe(1);
      expect(read.name).toBe(name);
    } finally {
      await dropProject(seed, projectId, [inbox.issueId], session);
    }
  }
);

test(
  specTitle(["INT-027"], "arrows and chevrons cycle requests with wrap and stay quiet while typing"),
  { tag: specTags(["INT-027"]) },
  async ({ driver, seed }) => {
    const tag = `NF258 nav ${Date.now()}`;
    const session = await signInSessionRetry(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N258"),
      { inboxView: true },
      session
    );
    const names = [`${tag} alpha request`, `${tag} beta request`, `${tag} gamma request`];
    const ids: string[] = [];
    for (const name of names) {
      ids.push((await serverCreateInboxIssue(seed.workspaceSlug, projectId, name, session)).issueId);
    }
    const all = new Set(ids);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await openOpenItem(driver, seed, projectId, ids[0]!, names[0]!);
      await expect.poll(() => driver.intakeDetailCurrentInboxIssueId(), { timeout: 60_000 }).toBe(ids[0]);

      await test.step("arrow-down cycles through every request and wraps", async () => {
        const seen = new Set<string>([ids[0]!]);
        let prev = ids[0]!;
        for (let i = 0; i < 3; i++) {
          await driver.intakeDetailPressArrow("down");
          await expect.poll(() => driver.intakeDetailCurrentInboxIssueId(), { timeout: 30_000 }).not.toBe(prev);
          prev = (await driver.intakeDetailCurrentInboxIssueId())!;
          seen.add(prev);
        }
        expect(seen).toEqual(all);
        expect(prev).toBe(ids[0]);
      });

      await test.step("arrow-up wraps from the first request", async () => {
        await driver.intakeDetailPressArrow("up");
        await expect.poll(() => driver.intakeDetailCurrentInboxIssueId(), { timeout: 30_000 }).not.toBe(ids[0]);
        const wrapped = await driver.intakeDetailCurrentInboxIssueId();
        expect(all.has(wrapped!)).toBe(true);
      });

      await test.step("chevrons move to neighbours and back", async () => {
        const start = await driver.intakeDetailCurrentInboxIssueId();
        await driver.intakeDetailClickChevron("next");
        await expect.poll(() => driver.intakeDetailCurrentInboxIssueId(), { timeout: 30_000 }).not.toBe(start);
        const moved = await driver.intakeDetailCurrentInboxIssueId();
        expect(all.has(moved!)).toBe(true);
        await driver.intakeDetailClickChevron("prev");
        await expect.poll(() => driver.intakeDetailCurrentInboxIssueId(), { timeout: 30_000 }).toBe(start);
      });

      await test.step("arrows do nothing while typing in the title or description", async () => {
        // The open request is identified by URL id: the rendered title
        // can lag the URL after a swap, so only id stability proves
        // the shortcut stayed quiet.
        const start = await driver.intakeDetailCurrentInboxIssueId();
        await driver.intakeDetailFocusTitle();
        await driver.intakeDetailPressArrow("down");
        await driver.intakeDetailPressArrow("up");
        expect(await driver.intakeDetailCurrentInboxIssueId()).toBe(start);
        await driver.intakeDetailFocusDescription();
        await driver.intakeDetailPressArrow("down");
        await driver.intakeDetailPressArrow("up");
        expect(await driver.intakeDetailCurrentInboxIssueId()).toBe(start);
      });
    } finally {
      await dropProject(seed, projectId, ids, session);
    }
  }
);

test(
  specTitle(["INT-028"], "deep-links open the request and tab, stale ids fall back, selection syncs the URL"),
  { tag: specTags(["INT-028"]) },
  async ({ driver, seed }) => {
    const tag = `NF258 deep ${Date.now()}`;
    const session = await signInSessionRetry(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N258"),
      { inboxView: true },
      session
    );
    const firstName = `${tag} first request`;
    const first = await serverCreateInboxIssue(seed.workspaceSlug, projectId, firstName, session);
    const secondName = `${tag} second request`;
    const second = await serverCreateInboxIssue(seed.workspaceSlug, projectId, secondName, session);
    const closedName = `${tag} closed request`;
    const closed = await serverCreateInboxIssue(seed.workspaceSlug, projectId, closedName, session);
    await serverIntakeDetailSetStatus(seed.workspaceSlug, projectId, closed.issueId, 1, session);
    const base = `/${seed.workspaceSlug}/projects/${projectId}/intake`;
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("an open-tab deep-link opens its request", async () => {
        await openOpenItem(driver, seed, projectId, second.issueId, secondName);
        expect(await driver.intakeDetailCurrentInboxIssueId()).toBe(second.issueId);
        expect(await driver.intakeDetailCurrentUrl()).toContain("currentTab=open");
      });

      await test.step("a closed-tab deep-link opens its request", async () => {
        await openClosedItem(driver, seed, projectId, closed.issueId, closedName);
        expect(await driver.intakeDetailCurrentInboxIssueId()).toBe(closed.issueId);
        expect(await driver.intakeDetailCurrentUrl()).toContain("currentTab=closed");
      });

      await test.step("a stale id falls back to the list", async () => {
        const stale = "00000000-0000-4000-8000-000000000000";
        await driver.intakeDetailOpenRaw(`${base}?currentTab=open&inboxIssueId=${stale}`);
        await expect.poll(() => driver.intakeDetailCurrentInboxIssueId(), { timeout: 60_000 }).not.toBe(stale);
        expect(await driver.intakeDetailListVisible()).toBe(true);
      });

      await test.step("selecting a request keeps the URL in sync", async () => {
        await openOpenItem(driver, seed, projectId, first.issueId, firstName);
        await driver.intakeDetailSelectListItem(secondName);
        await expect.poll(() => driver.intakeDetailCurrentInboxIssueId(), { timeout: 30_000 }).toBe(second.issueId);
        await expect.poll(() => driver.intakeDetailTitle(), { timeout: 30_000 }).toBe(secondName);
      });
    } finally {
      await dropProject(seed, projectId, [first.issueId, second.issueId, closed.issueId], session);
    }
  }
);

test(
  specTitle(["INT-028"], "a forbidden id redirects back to the list"),
  { tag: specTags(["INT-028"]) },
  async ({ driver, seed }) => {
    const tag = `NF258 forb ${Date.now()}`;
    const session = await signInSessionRetry(seed.email, seed.password);
    const guestCreds = requireGuest(seed);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N258"),
      { inboxView: true },
      session
    );
    const guestId = await seedGuestUserId(seed, session);
    await serverAddProjectMembers(seed.workspaceSlug, projectId, [{ memberId: guestId, role: 5 }], session);
    const name = `${tag} request`;
    const inbox = await serverCreateInboxIssue(seed.workspaceSlug, projectId, name, session);
    try {
      // No guest-view-all flag: the guest may not open another's request.
      await driver.rulesEnsureSignedIn(guestCreds.email, guestCreds.password, seed.workspaceSlug);
      await driver.intakeDetailOpenRaw(
        `/${seed.workspaceSlug}/projects/${projectId}/intake?currentTab=open&inboxIssueId=${inbox.issueId}`
      );
      await expect.poll(() => driver.intakeDetailCurrentInboxIssueId(), { timeout: 60_000 }).not.toBe(inbox.issueId);
      expect(await driver.intakeDetailCurrentUrl()).toContain("currentTab=open");
    } finally {
      await dropProject(seed, projectId, [inbox.issueId], session);
    }
  }
);

test(
  specTitle(["INT-029"], "description versions list, open for viewing, and restore into the editor"),
  { tag: specTags(["INT-029"]) },
  async ({ driver, seed }) => {
    const tag = `NF258 vers ${Date.now()}`;
    const session = await signInSessionRetry(seed.email, seed.password);
    const member = seed.mentionMember;
    if (!member) throw new Error("[parity] seed has no mentionMember; rerun parity-up.sh from a clean checkout.");
    const memberSession = await signInSessionRetry(member.email, member.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N258"),
      { inboxView: true },
      session
    );
    await serverAddProjectMembers(seed.workspaceSlug, projectId, [{ memberId: member.id, role: 15 }], session);
    // Same-user edits fold into one version row for ten minutes, so the
    // two revisions come from two authors: the member creates, the admin
    // revises.
    const firstBody = `alpha-first-revision-${Date.now()}`;
    const name = `${tag} request`;
    const created = await serverCreateInboxIssue(seed.workspaceSlug, projectId, name, memberSession);
    await serverIntakeDetailPatch(
      seed.workspaceSlug,
      projectId,
      created.issueId,
      { issue: { description_html: `<p>${firstBody}</p>` } },
      memberSession
    );
    const secondBody = `beta-second-revision-${Date.now()}`;
    // Gate on the member's version task: the admin edit must see the
    // first revision stored, or the two tasks race into three rows.
    await expect
      .poll(
        async () => {
          const rows = await serverIntakeDetailVersions(seed.workspaceSlug, projectId, created.issueId, session);
          if (rows.length === 0) return "";
          const full = await serverIntakeDetailVersion(
            seed.workspaceSlug,
            projectId,
            created.issueId,
            rows[0]!.id,
            session
          );
          return full.descriptionHtml;
        },
        { timeout: 120_000 }
      )
      .toContain(firstBody);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await openOpenItem(driver, seed, projectId, created.issueId, name);

      await test.step("a second author's edit grows the version list", async () => {
        await expect.poll(() => driver.intakeDetailVersionsVisible(), { timeout: 30_000 }).toBe(true);
        await driver.intakeDetailSetDescription(secondBody);
        await expect
          .poll(async () => serverIntakeDetailVersions(seed.workspaceSlug, projectId, created.issueId, session), {
            timeout: 120_000,
          })
          .toHaveLength(2);
        // The list is fetched once per page mount, so re-enter before
        // reading it; a plain focus change would refetch for a human.
        await openOpenItem(driver, seed, projectId, created.issueId, name);
        await driver.intakeDetailOpenVersions();
        await expect.poll(() => driver.intakeDetailVersionTexts(), { timeout: 30_000 }).toHaveLength(2);
      });

      const indexWith = async (marker: string): Promise<number> => {
        const versions = await serverIntakeDetailVersions(seed.workspaceSlug, projectId, created.issueId, session);
        for (let index = 0; index < versions.length; index++) {
          const full = await serverIntakeDetailVersion(
            seed.workspaceSlug,
            projectId,
            created.issueId,
            versions[index]!.id,
            session
          );
          if (full.descriptionHtml.includes(marker)) return index;
        }
        throw new Error(`[parity] no version row carries ${marker}.`);
      };

      await test.step("each version opens for viewing with its stored text", async () => {
        const secondIndex = await indexWith(secondBody);
        await driver.intakeDetailOpenVersion(secondIndex);
        await expect.poll(() => driver.intakeDetailVersionModalText(), { timeout: 30_000 }).toContain(secondBody);
        await driver.intakeDetailCloseVersionModal();
        await driver.intakeDetailOpenVersions();
        const firstIndex = await indexWith(firstBody);
        await driver.intakeDetailOpenVersion(firstIndex);
        await expect.poll(() => driver.intakeDetailVersionModalText(), { timeout: 30_000 }).toContain(firstBody);
        // Leave the first revision open for the restore step below.
      });

      await test.step("restoring replaces the current description", async () => {
        await driver.intakeDetailRestoreVersion();
        await expect.poll(() => driver.intakeDetailDescriptionText(), { timeout: 30_000 }).toContain(firstBody);
        await expect
          .poll(
            async () =>
              (await serverIntakeDetailRead(seed.workspaceSlug, projectId, created.issueId, session)).descriptionHtml,
            { timeout: 30_000 }
          )
          .toContain(firstBody);
        const read = await serverIntakeDetailRead(seed.workspaceSlug, projectId, created.issueId, session);
        expect(read.descriptionHtml).not.toContain(secondBody);
      });
    } finally {
      await dropProject(seed, projectId, [created.issueId], session);
    }
  }
);
