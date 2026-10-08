// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-186): the draft preview panel under the
// section editor renders the in-progress edit against a real issue. BUG
// (NEWFRONT-193): submit 403s on CSRF, so the scenario locks in the
// inline CSRF failure and proves the intended draft render through the
// CSRF-paired API, including the member user-scope path and the tier
// gate on drafts. Gap: no overridable section spans several kinds, so
// the kind switcher never renders; the scheduler recipe is all locked,
// so the install-target draft shape is proven by AGT-030 instead.
// Row: AGT-031.
import { test, expect } from "../../fixtures";
import {
  createIssue,
  ensureProject,
  parityProjectIdentifier,
  serverPromptPreview,
  serverPromptPreviewStatus,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness, seatMember } from "../support";
import { EDITABLE_SECTION, LOCKED_SECTION, expectCard } from "./support";

const ROWS = ["AGT-031"];
const DRAFT_BODY = "Parity AGT-031 draft marker.\n\nRendered before saving.";

test(
  specTitle(ROWS, "bug: NEWFRONT-193 draft preview 403s CSRF; panel shape and the intended draft render hold"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace and member", async () =>
      schedulerHarness("parity-agt31"));
    const { owner, ownerSession, workspaceSlug, tag } = harness;
    const member = await seatMember(harness);

    const issueId = await test.step("owner prepares an issue to render against", async () => {
      const project = await ensureProject(
        workspaceSlug,
        ownerSession,
        `AGT31 Project ${tag}`,
        parityProjectIdentifier("AG31")
      );
      return (await createIssue(workspaceSlug, project.id, ownerSession, `AGT31 draft issue ${tag}`)).id;
    });

    await test.step("owner signs in and opens the personal editor", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.promptsOpen(workspaceSlug);
      await expectCard(driver, EDITABLE_SECTION, (candidate) => candidate.body !== "");
      await driver.promptsOpenSectionEditor(EDITABLE_SECTION, "user");
    });

    await test.step("a single-kind section shows no kind switcher and refuses empty targets", async () => {
      expect(await driver.promptsDraftPreviewKinds()).toEqual([]);
      expect(await driver.promptsDraftPreviewSubmitEnabled()).toBe(false);
    });

    await test.step("bug: the draft submit fails inline on CSRF", async () => {
      await driver.promptsEditorFill(DRAFT_BODY);
      await driver.promptsDraftPreviewSubmit(issueId);
      await expect
        .poll(async () => (await driver.promptsDraftPreviewResult()).error ?? "", { timeout: 30_000 })
        .not.toBe("");
      const result = await driver.promptsDraftPreviewResult();
      expect(result.error).toContain("CSRF Failed");
      expect(result.prompt).toBeNull();
      await driver.promptsEditorCancel();
    });

    await test.step("intended render: the API renders the draft in place of the saved section", async () => {
      const rendered = await serverPromptPreview(
        workspaceSlug,
        "coding-task",
        { issue_id: issueId, scope: "user", section_key: EDITABLE_SECTION, body: DRAFT_BODY },
        ownerSession
      );
      expect(rendered.prompt).toContain("Parity AGT-031 draft marker.");
      // Members may preview their own user-scope composition.
      const memberRendered = await serverPromptPreview(
        workspaceSlug,
        "coding-task",
        { issue_id: issueId, scope: "user", section_key: EDITABLE_SECTION, body: DRAFT_BODY },
        member.session
      );
      expect(memberRendered.prompt).toContain("Parity AGT-031 draft marker.");
    });

    await test.step("bad targets and ineligible sections fail at the API", async () => {
      const missing = await serverPromptPreviewStatus(
        workspaceSlug,
        "coding-task",
        { issue_id: "00000000-0000-0000-0000-000000000000" },
        ownerSession
      );
      expect(missing.status).toBe(404);
      const locked = await serverPromptPreviewStatus(
        workspaceSlug,
        "coding-task",
        { issue_id: issueId, scope: "user", section_key: LOCKED_SECTION, body: "Draft of a locked section." },
        ownerSession
      );
      expect(locked.status).toBe(403);
    });
  }
);
