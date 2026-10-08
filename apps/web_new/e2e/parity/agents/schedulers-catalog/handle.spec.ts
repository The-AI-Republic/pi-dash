// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-184): the definition handle derives from the
// display name until the user edits it by hand; a blank derivation forces
// manual entry; malformed handles are rejected inline; edit mode locks the
// field. Row: AGT-005.
// Note: derivation lives on the project-side create form (the catalog
// dialog takes the handle as typed); both halves are proven where they
// live, and cancelling stores nothing either way.
import { test, expect } from "../../fixtures";
import { ensureProject, parityProjectIdentifier, serverSchedulers } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";

const ROWS = ["AGT-005"];

test(
  specTitle(ROWS, "handle derives until hand-edited; blank forces manual entry; edit locks"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt5"));
    const { owner, ownerSession, workspaceSlug } = harness;

    const projectId = await test.step("owner prepares a project", async () => {
      const project = await ensureProject(
        workspaceSlug,
        ownerSession,
        `AGT5 Project ${workspaceSlug}`,
        parityProjectIdentifier("AG5")
      );
      return project.id;
    });

    await test.step("owner signs in and opens the catalog", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.schedulerOpenCatalog(workspaceSlug);
    });

    await test.step("the catalog dialog rejects malformed handles inline", async () => {
      const before = await serverSchedulers(workspaceSlug, ownerSession);
      await driver.schedulerOpenCreate();
      await driver.schedulerFillDefinition({
        name: "AGT5 Whatever",
        handle: "Bad Handle!!",
        prompt: "Never stored.",
      });
      await driver.schedulerSubmitDefinition();
      await expect
        .poll(() => driver.schedulerDefinitionErrors(), { timeout: 30_000 })
        .toContain("Use lowercase letters, numbers, and dashes only.");
      expect(await driver.schedulerDefinitionOpen()).toBe(true);
      const after = await serverSchedulers(workspaceSlug, ownerSession);
      expect(after.map((row) => row.slug).sort()).toEqual(before.map((row) => row.slug).sort());
      await driver.schedulerCloseDefinition();
    });

    await test.step("a name proposes a handle on the project-side create form", async () => {
      await driver.schedulerOpenProjectSchedulers(workspaceSlug, projectId);
      await driver.schedulerOpenProjectCreate();
      await driver.schedulerProjectCreateFillName("My Audit Job");
      await expect.poll(() => driver.schedulerProjectCreateHandleValue(), { timeout: 30_000 }).toBe("my-audit-job");
      await driver.schedulerProjectCreateFillName("Second Thing 2!");
      await expect.poll(() => driver.schedulerProjectCreateHandleValue(), { timeout: 30_000 }).toBe("second-thing-2");
    });

    await test.step("a blank derivation forces manual entry", async () => {
      await driver.schedulerProjectCreateFillName("!!!");
      await expect.poll(() => driver.schedulerProjectCreateHandleValue(), { timeout: 30_000 }).toBe("");
      await driver.schedulerProjectCreateSubmit();
      await expect
        .poll(() => driver.schedulerProjectCreateErrors(), { timeout: 30_000 })
        .toContain("Slug is required.");
      const stored = await serverSchedulers(workspaceSlug, ownerSession);
      expect(stored.some((row) => row.name === "!!!")).toBe(false);
    });

    await test.step("a hand-edited handle stops following the name", async () => {
      await driver.schedulerProjectCreateFillName("Final Name");
      await expect.poll(() => driver.schedulerProjectCreateHandleValue(), { timeout: 30_000 }).toBe("final-name");
      await driver.schedulerProjectCreateFillHandle("custom-kept");
      await driver.schedulerProjectCreateFillName("Something Else Entirely");
      // Derivation runs synchronously off the name change, so a short pause
      // plus an unchanged read proves the hand-edited handle stopped
      // following (had it followed, it would read "something-else-entirely").
      await new Promise((resolve) => setTimeout(resolve, 2000));
      expect(await driver.schedulerProjectCreateHandleValue()).toBe("custom-kept");
    });

    await test.step("cancelling stores nothing; edit mode locks the field", async () => {
      await driver.schedulerCloseProjectCreate();
      const stored = await serverSchedulers(workspaceSlug, ownerSession);
      expect(stored.some((row) => row.slug === "custom-kept")).toBe(false);
      await driver.schedulerOpenCatalog(workspaceSlug);
      await driver.schedulerOpenEdit("security-audit");
      expect(await driver.schedulerDefinitionValues()).toMatchObject({ handle: "security-audit" });
      expect(await driver.schedulerDefinitionHandleLocked()).toBe(true);
      await driver.schedulerCloseDefinition();
    });
  }
);
