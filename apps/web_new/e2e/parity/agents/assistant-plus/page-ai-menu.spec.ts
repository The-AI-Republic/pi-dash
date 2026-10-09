// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-188): the page editor loads and edits, but
// offers no AI entry — no block handle reveals on hover and no Ask Pi
// menu exists — and the page session fires no rephrase calls.
// bug: NEWFRONT-206 — the "ai" extension sits in the editor's disabled
// list, and behind it the Ask Pi submit is unwired (write-only query
// box, keystrokes dismiss the popup) with no rephrase backend route.
// Intended is a typed request that posts, renders the returned markup
// for review, and inserts on confirmation. Row: AGT-063.
import { test, expect } from "../../fixtures";
import { serverCreatePage, serverCreateProject, serverRephraseStatus } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";

const ROWS = ["AGT-063"];

test(
  specTitle(ROWS, "bug: NEWFRONT-206 page editor offers no AI entry; rephrase backend missing"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt63p"));
    const { owner, workspaceSlug, tag } = harness;
    const projectId = await test.step("owner creates a project and a page", async () => {
      const project = await serverCreateProject(
        workspaceSlug,
        harness.ownerSession,
        `Parity AGT63P ${tag}`,
        `AP${tag.slice(0, 3).toUpperCase()}`
      );
      return project;
    });
    const pageId = await serverCreatePage(workspaceSlug, projectId, harness.ownerSession, `Parity AI page ${tag}`);

    await test.step("rephrase endpoint has no backend route", async () => {
      expect(await serverRephraseStatus(workspaceSlug, harness.ownerSession)).toBe(404);
    });

    await test.step("owner signs in and opens the page editor", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.pageEditorOpen(workspaceSlug, projectId, pageId);
    });

    await test.step("bug: no AI handle reveals on block hover", async () => {
      expect(await driver.pageEditorAiHandleCount()).toBe(0);
      expect(await driver.pageEditorAiMenuVisible()).toBe(false);
      expect(await driver.pageEditorRephraseRequests()).toEqual([]);
    });
  }
);
