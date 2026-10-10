// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-257): intake create dialog — the header
// entry point, required title plus rich description plus inline
// properties, create-time validation, create-more mode, the busy-editor
// dismiss guard, creation attachments, and the triage-state guarantee.
// The dialog drives the intake-create endpoint, which files the request
// in the project's triage state as an in-app source and logs a creation
// activity; the screen then opens the new item.
// Rows: INT-013, INT-014, INT-015, INT-016, INT-017, INT-033.
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverCleanupProject,
  serverCreateProjectWithFlags,
  serverHistory,
  serverInboxIssues,
  serverIntakeCreateDeleteTriage,
  serverIntakeCreateDetail,
  serverIntakeCreateIssueAssets,
  serverIntakeCreateRaw,
  serverIntakeCreateTriageProbe,
  serverIntakeStates,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";

/** One-pixel PNG payload for the editor image-insert path. */
function tinyPng(): Buffer {
  return Buffer.from(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==",
    "base64"
  );
}

/** Fresh intake-enabled project owned by the seeded owner. */
async function scratchProject(seed: ParitySeedFacts, tag: string, session: string): Promise<string> {
  return serverCreateProjectWithFlags(
    seed.workspaceSlug,
    `${tag} project`,
    parityProjectIdentifier("N257"),
    {
      inboxView: true,
    },
    session
  );
}

/** The created request's issue id out of the post-create address. */
function inboxIssueIdFromPath(path: string): string {
  const match = /[?&]inboxIssueId=([^&]+)/.exec(path);
  if (!match?.[1]) throw new Error(`[parity] post-create path carries no inboxIssueId: ${path}.`);
  return decodeURIComponent(match[1]);
}

/** Open the dialog on a fresh project as the seeded owner. */
async function openFreshDialog(
  driver: ParityDriver,
  seed: ParitySeedFacts,
  tag: string,
  session: string
): Promise<string> {
  const projectId = await scratchProject(seed, tag, session);
  await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
  await driver.intakeCreateOpen(seed.workspaceSlug, projectId);
  return projectId;
}

test(
  specTitle(["INT-013"], "create files the request in triage as in-app, logs activity, opens the item"),
  { tag: specTags(["INT-013"]) },
  async ({ driver, seed }) => {
    const tag = `NF257 create ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await openFreshDialog(driver, seed, tag, session);
    try {
      const title = `${tag} request`;
      const body = `${tag} description`;

      await test.step("dialog opens on the project triage state", async () => {
        expect(await driver.intakeStateValue()).toBe("Triage");
      });

      await test.step("fill and save", async () => {
        await driver.intakeCreateFillTitle(title);
        await driver.intakeCreateFillDescription(body);
        await driver.intakeCreateSetPriority("High");
        await driver.intakeCreateSubmit();
      });

      const issueId = inboxIssueIdFromPath(await driver.currentPath());

      await test.step("screen opens the new item", async () => {
        expect(await driver.intakeCreateDialogOpen()).toBe(false);
        await expect.poll(() => driver.hasVisibleText(title), { timeout: 30_000 }).toBe(true);
      });

      await test.step("server stored a pending in-app request in triage", async () => {
        const triage = await serverIntakeStates(seed.workspaceSlug, projectId, session);
        const detail = await serverIntakeCreateDetail(seed.workspaceSlug, projectId, issueId, session);
        expect(detail.name).toBe(title);
        expect(detail.source).toBe("IN_APP");
        expect(detail.status).toBe(-2);
        expect(detail.priority).toBe("high");
        expect(detail.stateId).toBe(triage[0]?.id ?? "");
        expect(detail.descriptionHtml).toContain(body);
      });

      await test.step("a creation activity is logged", async () => {
        // The creation entry lands through the background worker under
        // the property filter (the unfiltered history read 500s — old
        // bug, outside these rows).
        await expect
          .poll(
            async () =>
              (
                await serverHistory(seed.workspaceSlug, projectId, issueId, session, "?activity_type=issue-property")
              ).entries.some((entry) => entry.verb === "created"),
            { timeout: 90_000 }
          )
          .toBe(true);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["INT-014"], "over-long title blocks submission client-side"),
  { tag: specTags(["INT-014"]) },
  async ({ driver, seed }) => {
    const tag = `NF257 longtitle ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await openFreshDialog(driver, seed, tag, session);
    try {
      await test.step("short title submits normally", async () => {
        await driver.intakeCreateFillTitle(`${tag} ok`);
        expect(await driver.intakeCreateTitleHint()).toBe("");
        expect(await driver.intakeCreateSubmitDisabled()).toBe(false);
      });

      await test.step("past 255 characters the submit locks with a warning", async () => {
        await driver.intakeCreateFillTitle("x".repeat(256));
        expect(await driver.intakeCreateTitleHint()).not.toBe("");
        expect(await driver.intakeCreateSubmitDisabled()).toBe(true);
      });

      await test.step("shortening re-enables submission", async () => {
        await driver.intakeCreateFillTitle(`${tag} ok`);
        expect(await driver.intakeCreateTitleHint()).toBe("");
        expect(await driver.intakeCreateSubmitDisabled()).toBe(false);
      });

      await test.step("nothing was created", async () => {
        const rows = await serverInboxIssues(seed.workspaceSlug, projectId, session);
        expect(rows).toEqual([]);
        await driver.intakeCreateDiscard();
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["INT-014"], "empty title and out-of-range priority are refused by the server"),
  { tag: specTags(["INT-014"]) },
  async ({ seed }) => {
    const tag = `NF257 refusal ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N257"),
      { inboxView: true },
      session
    );
    try {
      await test.step("empty title answers 400", async () => {
        const refused = await serverIntakeCreateRaw(seed.workspaceSlug, projectId, { name: "" }, session);
        expect(refused.status).toBe(400);
      });

      await test.step("out-of-range priority answers 400", async () => {
        const refused = await serverIntakeCreateRaw(
          seed.workspaceSlug,
          projectId,
          { name: `${tag} bad priority`, priority: "bogus" },
          session
        );
        expect(refused.status).toBe(400);
      });

      await test.step("nothing was created", async () => {
        const rows = await serverInboxIssues(seed.workspaceSlug, projectId, session);
        expect(rows).toEqual([]);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["INT-015"], "create-more stays open and resets the form"),
  { tag: specTags(["INT-015"]) },
  async ({ driver, seed }) => {
    const tag = `NF257 more ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await openFreshDialog(driver, seed, tag, session);
    try {
      const first = `${tag} first`;

      await test.step("toggle create-more on", async () => {
        expect(await driver.intakeCreateMoreOn()).toBe(false);
        await driver.intakeCreateToggleMore();
        expect(await driver.intakeCreateMoreOn()).toBe(true);
      });

      await test.step("saving keeps the dialog with a cleared form", async () => {
        await driver.intakeCreateFillTitle(first);
        await driver.intakeCreateFillDescription(`${tag} body`);
        await driver.intakeCreateSetPriority("Low");
        await driver.intakeCreateSubmitStayingOpen();
        expect(await driver.intakeCreateDialogOpen()).toBe(true);
        expect(await driver.intakeCreateTitleValue()).toBe("");
        expect(await driver.intakeCreateDescriptionText()).toBe("");
        expect(await driver.intakeCreatePriorityValue()).toBe("None");
      });

      await test.step("the first request was still created", async () => {
        const rows = await serverInboxIssues(seed.workspaceSlug, projectId, session);
        expect(rows.map((row) => row.name)).toEqual([first]);
      });

      await test.step("a second request saves from the reset form", async () => {
        const second = `${tag} second`;
        await driver.intakeCreateFillTitle(second);
        await driver.intakeCreateSubmitStayingOpen();
        // The completion toast may still show the first save, so the
        // second write is awaited on the server rows, not the toast.
        await expect
          .poll(async () => (await serverInboxIssues(seed.workspaceSlug, projectId, session)).map((row) => row.name), {
            timeout: 30_000,
          })
          .toEqual(expect.arrayContaining([first, second]));
        await driver.intakeCreateDiscard();
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["INT-015"], "without create-more saving closes and opens the item"),
  { tag: specTags(["INT-015"]) },
  async ({ driver, seed }) => {
    const tag = `NF257 single ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await openFreshDialog(driver, seed, tag, session);
    try {
      const title = `${tag} request`;
      expect(await driver.intakeCreateMoreOn()).toBe(false);
      await driver.intakeCreateFillTitle(title);
      await driver.intakeCreateSubmit();
      expect(await driver.intakeCreateDialogOpen()).toBe(false);
      const issueId = inboxIssueIdFromPath(await driver.currentPath());
      const detail = await serverIntakeCreateDetail(seed.workspaceSlug, projectId, issueId, session);
      expect(detail.name).toBe(title);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["INT-016"], "discard waits while the editor is busy, then closes once settled"),
  { tag: specTags(["INT-016"]) },
  async ({ driver, seed }) => {
    const tag = `NF257 discardbusy ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await openFreshDialog(driver, seed, tag, session);
    try {
      await test.step("drive the editor busy with a held upload", async () => {
        await driver.intakeCreateFillTitle(`${tag} request`);
        await driver.intakeCreateStallNextUpload();
        await driver.intakeCreateInsertImage({ name: "dot.png", mimeType: "image/png", bytes: tinyPng() });
      });

      await test.step("discard warns and keeps the dialog", async () => {
        await driver.intakeCreateDiscard();
        await expect.poll(() => driver.isToastVisible("still processing"), { timeout: 15_000 }).toBe(true);
        expect(await driver.intakeCreateDialogOpen()).toBe(true);
      });

      await test.step("settling then discarding closes", async () => {
        await driver.intakeCreateReleaseUpload();
        await expect.poll(() => driver.intakeCreateEditorImageCount(), { timeout: 60_000 }).toBe(1);
        await driver.intakeCreateDiscard();
        await expect.poll(() => driver.intakeCreateDialogOpen(), { timeout: 15_000 }).toBe(false);
      });

      await test.step("nothing was created", async () => {
        const rows = await serverInboxIssues(seed.workspaceSlug, projectId, session);
        expect(rows).toEqual([]);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["INT-016"], "bug: escape while the editor is busy closes the dialog (NEWFRONT-304)"),
  { tag: specTags(["INT-016"]) },
  async ({ driver, seed }) => {
    const tag = `NF257 escbusy ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await openFreshDialog(driver, seed, tag, session);
    try {
      // Locks in the old bug: the dialog shell's unconditional Escape
      // handler wins over the form's busy guard, so Escape mid-upload
      // closes with no wait notice. Intended: same wait notice as
      // Discard, dialog kept. See NEWFRONT-304 and the INT-016 row.
      await driver.intakeCreateFillTitle(`${tag} request`);
      await driver.intakeCreateStallNextUpload();
      await driver.intakeCreateInsertImage({ name: "dot.png", mimeType: "image/png", bytes: tinyPng() });
      await driver.intakeCreatePressEscape();
      await expect.poll(() => driver.intakeCreateDialogOpen(), { timeout: 15_000 }).toBe(false);
      await driver.intakeCreateReleaseUpload();
      const rows = await serverInboxIssues(seed.workspaceSlug, projectId, session);
      expect(rows).toEqual([]);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["INT-016"], "escape dismisses the settled dialog"),
  { tag: specTags(["INT-016"]) },
  async ({ driver, seed }) => {
    const tag = `NF257 escape ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await openFreshDialog(driver, seed, tag, session);
    try {
      await driver.intakeCreateFillTitle(`${tag} request`);
      await driver.intakeCreatePressEscape();
      await expect.poll(() => driver.intakeCreateDialogOpen(), { timeout: 15_000 }).toBe(false);
      const rows = await serverInboxIssues(seed.workspaceSlug, projectId, session);
      expect(rows).toEqual([]);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["INT-017"], "files added while composing attach to the created request"),
  { tag: specTags(["INT-017"]) },
  async ({ driver, seed }) => {
    const tag = `NF257 attach ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await openFreshDialog(driver, seed, tag, session);
    try {
      const title = `${tag} request`;

      await test.step("compose with an image", async () => {
        await driver.intakeCreateFillTitle(title);
        await driver.intakeCreateFillDescription(`${tag} body`);
        await driver.intakeCreateInsertImage({ name: "dot.png", mimeType: "image/png", bytes: tinyPng() });
        await expect.poll(() => driver.intakeCreateEditorImageCount(), { timeout: 60_000 }).toBe(1);
      });

      const issueId = await test.step("save and resolve the created item", async () => {
        await driver.intakeCreateSubmit();
        return inboxIssueIdFromPath(await driver.currentPath());
      });

      await test.step("the upload is attached to the created work item", async () => {
        const assets = await serverIntakeCreateIssueAssets(issueId);
        expect(assets).toHaveLength(1);
        expect(assets[0]?.entityType).toBe("ISSUE_DESCRIPTION");
        expect(assets[0]?.uploaded).toBe(true);
        const detail = await serverIntakeCreateDetail(seed.workspaceSlug, projectId, issueId, session);
        expect(detail.name).toBe(title);
        expect(detail.descriptionHtml).toContain("image-component");
        expect(detail.descriptionHtml).toContain(assets[0]?.id ?? "");
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["INT-033"], "the picker resolves to triage and first create recreates a missing triage state"),
  { tag: specTags(["INT-033"]) },
  async ({ driver, seed }) => {
    const tag = `NF257 triage ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N257"),
      { inboxView: true },
      session
    );
    try {
      await test.step("fresh project carries one triage state", async () => {
        const probe = await serverIntakeCreateTriageProbe(seed.workspaceSlug, projectId, session);
        expect(probe.status).toBe(200);
        expect(probe.name).toBe("Triage");
      });

      await test.step("remove it so auto-creation is observable", async () => {
        const deleted = await serverIntakeCreateDeleteTriage(projectId);
        expect(deleted).toBe(1);
        const missing = await serverIntakeCreateTriageProbe(seed.workspaceSlug, projectId, session);
        expect(missing.status).toBe(404);
      });

      await test.step("create the first request through the dialog", async () => {
        await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
        await driver.intakeCreateOpen(seed.workspaceSlug, projectId);
        await driver.intakeCreateFillTitle(`${tag} request`);
        await driver.intakeCreateSubmit();
      });

      const issueId = inboxIssueIdFromPath(await driver.currentPath());

      await test.step("a triage state exists again and owns the request", async () => {
        const probe = await serverIntakeCreateTriageProbe(seed.workspaceSlug, projectId, session);
        expect(probe.status).toBe(200);
        expect(probe.name).toBe("Triage");
        const detail = await serverIntakeCreateDetail(seed.workspaceSlug, projectId, issueId, session);
        expect(detail.stateId).toBe(probe.id ?? "");
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
