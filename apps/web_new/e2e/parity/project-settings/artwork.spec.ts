// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-389): emblem and cover artwork on the General
// tab — the emblem picker swaps the project mark, the cover picker offers
// the bundled gallery plus an upload tab, and both persist through the
// normal save. Rows: PSET-007.
//
// Observed behavior notes: the emblem opens an emoji/material picker and
// marks the form dirty without saving on its own; the cover popover tabs
// are Images (bundled gallery, click applies straight into the form),
// Upload (dropzone plus Upload & Save) and Unsplash (only when the
// instance has it configured — the scratch stack does not, so the oracle
// asserts Images and Upload only). TARGET (main#523, OPEN): cover control,
// gallery and search are removed — the cover steps below pin current
// behavior and collapse to emblem-only once it merges.
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverCleanupProject,
  serverCreateProjectWithFlags,
  serverProjectRecord,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

/** One-pixel PNG as base64 for the upload path. */
const PIXEL_PNG_BASE64 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

test(
  specTitle(["PSET-007"], "emblem picker swaps the mark and the save persists it"),
  { tag: specTags(["PSET-007"]) },
  async ({ driver, seed }) => {
    const tag = `NF389 emblem ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N389"),
      {},
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsGeneralOpen(seed.workspaceSlug, projectId);

      await driver.settingsGeneralEmblemOpen();
      expect(await driver.settingsGeneralEmblemPickerVisible()).toBe(true);
      await driver.settingsGeneralEmblemPick("🚀");
      await driver.settingsGeneralSave();
      // The mark persists as decimal unified codepoints ("128640" for
      // U+1F680), not the raw character.
      const unified = [..."🚀"].map((part) => String(part.codePointAt(0))).join("-");
      await expect
        .poll(
          async () =>
            JSON.stringify((await serverProjectRecord(seed.workspaceSlug, projectId, session))["logo_props"] ?? ""),
          { timeout: 30_000 }
        )
        .toContain(unified);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PSET-007"], "cover gallery pick and file upload persist through the save"),
  { tag: specTags(["PSET-007"]) },
  async ({ driver, seed }) => {
    const tag = `NF389 cover ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N389"),
      {},
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsGeneralOpen(seed.workspaceSlug, projectId);
      const before = String(
        (await serverProjectRecord(seed.workspaceSlug, projectId, session))["cover_image_url"] ?? ""
      );

      await test.step("the picker offers the bundled gallery and the upload tab", async () => {
        // TARGET (main#523): no cover control and no gallery search render.
        await driver.settingsGeneralCoverOpen();
        const tabs = await driver.settingsGeneralCoverTabNames();
        expect(tabs).toContain("Images");
        expect(tabs).toContain("Upload");
        expect(await driver.settingsGeneralCoverGalleryCount()).toBeGreaterThan(0);
      });

      await test.step("a gallery pick persists on save", async () => {
        await driver.settingsGeneralCoverPickGallery(3);
        expect(await driver.settingsGeneralCoverVisible()).toBe(true);
        await driver.settingsGeneralSave();
        await expect
          .poll(
            async () =>
              String((await serverProjectRecord(seed.workspaceSlug, projectId, session))["cover_image_url"] ?? ""),
            { timeout: 30_000 }
          )
          .not.toBe(before);
      });

      await test.step("an uploaded file persists on save", async () => {
        await driver.settingsGeneralCoverOpen();
        await driver.settingsGeneralCoverUpload({
          name: `${tag}.png`,
          mime: "image/png",
          bytes: PIXEL_PNG_BASE64,
        });
        await driver.settingsGeneralSave();
        const after = String(
          (await serverProjectRecord(seed.workspaceSlug, projectId, session))["cover_image_url"] ?? ""
        );
        expect(after).not.toBe("");
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
