// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-383): page-export dialog plus cross-cutting
// export rows. Rows EXP-016 (dialog open + defaults), EXP-017 (format
// choice + conditional page size), EXP-018 (content scope), EXP-019
// (paginated document download), EXP-020 (plain-text download), EXP-021
// (feedback + reset), EXP-023 (keyboard + focus), EXP-024 (uniform
// behavior across editions and desktop). Green on apps/web first, except
// the NEWFRONT-951 re-pins below: they assert the intended behavior the
// new app builds, so they fail on the old app by design.
//
// Fixture shape: page scenarios create pages in the seeded project
// through the API with per-attempt unique names (additive only; shared
// seed rows are never mutated), open the detail authenticated, wait for
// sync, and type body content through the live editor; pictures arrive
// via the editor image upload (stack-served asset) and via pasting the
// same asset under its absolute URL (the only form whose raster the
// document renderer embeds). Cross-surface scenarios mint their own
// owner and workspace following the established harness pattern.
//
// Live-verified notes (oracle-prod): the dialog offers exactly the
// PDF/Markdown formats, the Everything/No-images scopes, and the A4, A3,
// A2, Letter, Legal, Tabloid page sizes, defaulting to PDF/Everything/A4
// with the size row rendered only for PDF. The document renderer embeds
// the product typeface subset, sizes the page media box to the chosen
// size, and embeds fetchable absolute-URL pictures as image objects while
// the image-free scope drops them; both renders are fully client-side
// (no export-shaped API traffic).
//
// Re-pinned to the intended behavior (NEWFRONT-951; these scenarios fail
// on the old app, where the bugs live): the full markdown scope emits
// `![](url)` and the image-free scope drops the image line while keeping
// the text (old-app bug NEWFRONT-356); the full-fidelity document scope
// embeds uploaded pictures (old-app bug NEWFRONT-470); every export
// control is Tab-reachable with a visible focus indicator and the dialog
// menus navigate and select by keyboard (old-app bug NEWFRONT-471).
//
// Content-bearing exports re-roll the load until the marker lands (bug
// NEWFRONT-360: the shared editor reference can lose its load race and
// export empty content).
import { pdfFacts } from "./pdfFacts";
import { test, expect } from "../fixtures";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";
import {
  browserSessionCookies,
  createIssue,
  createProjectViaApi,
  createWorkspaceForProjects,
  drainWorkspaceSeed,
  markOnboardedForProjects,
  parityProjectIdentifier,
  serverCreatePage,
  setLastWorkspaceForProjects,
  signUpAuthedSession,
  uniqueSuffixForProjects,
  type AuthedSession,
} from "../helpers/api";
import {
  browserSessionCookies as editionBrowserCookies,
  isCloudEdition,
  seededSession,
} from "../workspace-settings/edition-session";
import { isParityApiRequestPath } from "../helpers/api-traffic";
import { specTags, specTitle } from "../helpers/tags";

const ROW_016 = ["EXP-016"];
const ROW_017 = ["EXP-017"];
const ROW_018 = ["EXP-018"];
const ROW_019 = ["EXP-019"];
const ROW_020 = ["EXP-020"];
const ROW_021 = ["EXP-021"];
const ROW_023 = ["EXP-023"];
const ROW_024 = ["EXP-024"];

/** One-pixel PNG bytes for the image-upload fixture. */
const PIXEL_PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==",
  "base64"
);

/** Per-attempt unique marker so oracle retries never collide on page names. */
function stamp(): string {
  return Date.now().toString(36);
}

function detailPath(seed: ParitySeedFacts, projectId: string, pageId: string): string {
  return `/${seed.workspaceSlug}/projects/${projectId}/pages/${pageId}`;
}

async function seedSession(seed: ParitySeedFacts): Promise<string> {
  // Cloud stacks seed a passwordless identity that enters through the
  // provider's dev-login endpoint; OSS seeds sign in through the
  // credential form. Same session-cookie contract either way.
  return seededSession(seed, seed.email);
}

/**
 * Open a seeded-project page as the seed user after world-owner steps ran
 * in this context. The world owner enters through the sign-up form (a
 * session-id cookie) while the cloud seed enters through dev-login (a
 * pidash_access cookie); without a clear both identities ride along and
 * the server answers as the wrong one. On OSS both sessions are
 * session-id, so the clear changes nothing there.
 */
async function openSeedPageAfterWorld(
  driver: ParityDriver,
  path: string,
  session: string,
  seed: ParitySeedFacts
): Promise<void> {
  await driver.page.context().clearCookies();
  await driver.openAuthenticated(path, editionBrowserCookies(session, seed));
}

/** Filename slug the dialog derives from a page title (verified live). */
function pageTitleSlug(title: string): string {
  return title
    .toLowerCase()
    .replace(/[^a-z0-9-_]/g, "-")
    .replace(/-+/g, "-");
}

/** Paste an image element into the focused live editor (stable; see the pages-area precedent). */
async function pasteImage(driver: ParityDriver, imageSrc: string): Promise<void> {
  await driver.page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
  await driver.page.evaluate((src) => {
    const blob = new Blob([`<img src="${src}">`], { type: "text/html" });
    return navigator.clipboard.write([new ClipboardItem({ "text/html": blob })]);
  }, imageSrc);
  await driver.page.keyboard.press("ControlOrMeta+V");
}

/** Poll until a visible toast mentions `snippet` (toasts auto-dismiss). */
async function waitForToast(driver: ParityDriver, snippet: string, timeoutMs = 60_000): Promise<void> {
  await expect
    .poll(
      async () => {
        const toast = await driver.rulesLastToast();
        if (toast === null) return "";
        return `${toast.title} ${toast.message}`;
      },
      { timeout: timeoutMs }
    )
    .toContain(snippet);
}

/** Capture API request paths in the background; the scenario asserts the set. */
function captureApiPaths(driver: ParityDriver): { stop: () => string[] } {
  const seen: string[] = [];
  const onRequest = (request: { url(): string }): void => {
    const url = request.url();
    // Endpoint calls only: the classifier keeps dev-server module fetches
    // (which the oracle's production build never emits) out of the set.
    if (!isParityApiRequestPath(url)) return;
    seen.push(new URL(url).pathname);
  };
  driver.page.on("request", onRequest);
  return {
    stop: () => {
      driver.page.off("request", onRequest);
      return [...new Set(seen)].sort();
    },
  };
}

/** Open an export dialog over a fresh seeded-project page; resolves with its title facts. */
async function openExportDialog(
  driver: ParityDriver,
  seed: ParitySeedFacts,
  label: string
): Promise<{ title: string; pageId: string; session: string }> {
  const tag = stamp();
  const session = await seedSession(seed);
  const title = `${label} ${tag}`;
  const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, title);
  await driver.openAuthenticated(detailPath(seed, seed.projectId, pageId), editionBrowserCookies(session, seed));
  await driver.pagesLiveWaitSynced();
  await driver.pagesHeaderOpenOptions();
  await driver.pagesHeaderChooseMenuItem("Export");
  await expect.poll(() => driver.pagesHeaderExportDialogVisible(), { timeout: 15_000 }).toBe(true);
  return { title, pageId, session };
}

/** Reopen the export dialog on the current page (the dialog was closed). */
async function reopenExportDialog(driver: ParityDriver): Promise<void> {
  await driver.pagesHeaderOpenOptions();
  await driver.pagesHeaderChooseMenuItem("Export");
  await expect.poll(() => driver.pagesHeaderExportDialogVisible(), { timeout: 15_000 }).toBe(true);
}

/**
 * Type a fresh marker per attempt and export plain text until the marker
 * lands (bug NEWFRONT-360: the shared editor reference can lose its load
 * race and export empty content). Resolves with the landed marker.
 */
async function landPageContent(
  driver: ParityDriver,
  seed: ParitySeedFacts,
  projectId: string,
  pageId: string,
  session: string,
  tag: string
): Promise<string> {
  let marker = "";
  for (let attempt = 0; attempt < 5; attempt += 1) {
    marker = `land-${tag}-a${attempt}`;
    await driver.openAuthenticated(detailPath(seed, projectId, pageId), editionBrowserCookies(session, seed));
    await driver.pagesLiveWaitSynced();
    await driver.pagesLiveTypeInBody(`${marker} `);
    await expect.poll(() => driver.pagesLiveBodyText(), { timeout: 60_000 }).toContain(marker);
    await driver.pagesHeaderOpenOptions();
    await driver.pagesHeaderChooseMenuItem("Export");
    await expect.poll(() => driver.pagesHeaderExportDialogVisible(), { timeout: 15_000 }).toBe(true);
    await driver.pagesHeaderExportSetFormat("Markdown");
    const file = await driver.pageExportConfirmFull();
    await expect.poll(() => driver.pagesHeaderExportDialogVisible(), { timeout: 30_000 }).toBe(false);
    if (file.content.includes(marker)) return marker;
  }
  throw new Error(`[parity] page content never landed for ${tag} (NEWFRONT-360).`);
}

test(
  specTitle(ROW_016, "dialog opens over the page offering format, scope and page-size rows at sensible defaults"),
  { tag: specTags(ROW_016) },
  async ({ driver, seed }) => {
    test.setTimeout(600_000);
    const { title } = await openExportDialog(driver, seed, "Dlg016");

    expect(await driver.pageExportHeading()).toBe("Export page");
    expect(await driver.pagesHeaderExportForm()).toEqual({
      format: "PDF",
      scope: "Everything",
      pageFormat: "A4",
      pageFormatVisible: true,
    });
    expect(await driver.pageExportOfferedOptions("format")).toEqual(["PDF", "Markdown"]);
    await expect.poll(() => driver.pagesHeaderExportDialogVisible(), { timeout: 15_000 }).toBe(true);
    expect(await driver.pageExportOfferedOptions("scope")).toEqual(["Everything", "No images"]);
    await expect.poll(() => driver.pagesHeaderExportDialogVisible(), { timeout: 15_000 }).toBe(true);
    expect(await driver.pageExportOfferedOptions("pageFormat")).toEqual([
      "A4",
      "A3",
      "A2",
      "Letter",
      "Legal",
      "Tabloid",
    ]);

    // The page stays mounted behind the dialog.
    await expect.poll(() => driver.pagesHeaderExportDialogVisible(), { timeout: 15_000 }).toBe(true);
    expect(await driver.pagesLiveTitleText()).toBe(title);
    await driver.pagesHeaderExportCancel();
    await expect.poll(() => driver.pagesHeaderExportDialogVisible(), { timeout: 15_000 }).toBe(false);
  }
);

test(
  specTitle(ROW_017, "page-size row shows for the document format and hides for plain text"),
  { tag: specTags(ROW_017) },
  async ({ driver, seed }) => {
    test.setTimeout(600_000);
    await openExportDialog(driver, seed, "Dlg017");

    await test.step("the document format offers the office page sizes", async () => {
      expect(await driver.pageExportOfferedOptions("pageFormat")).toEqual([
        "A4",
        "A3",
        "A2",
        "Letter",
        "Legal",
        "Tabloid",
      ]);
    });

    await test.step("plain text hides the page-size row and keeps the size choice", async () => {
      await driver.pagesHeaderExportSetPageFormat("Letter");
      expect((await driver.pagesHeaderExportForm()).pageFormat).toBe("Letter");
      await driver.pagesHeaderExportSetFormat("Markdown");
      const hidden = await driver.pagesHeaderExportForm();
      expect(hidden.format).toBe("Markdown");
      expect(hidden.pageFormatVisible).toBe(false);
      expect(await driver.pageExportOfferedOptions("pageFormat")).toEqual([]);
      await driver.pagesHeaderExportSetFormat("PDF");
      const restored = await driver.pagesHeaderExportForm();
      expect(restored.pageFormatVisible).toBe(true);
      expect(restored.pageFormat).toBe("Letter");
    });

    await driver.pagesHeaderExportCancel();
  }
);

test(
  specTitle(ROW_018, "image-free scope strips pictures from the document while keeping the text"),
  { tag: specTags(ROW_018) },
  async ({ driver, seed }) => {
    test.setTimeout(600_000);
    const tag = stamp();
    const session = await seedSession(seed);
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Scope018 ${tag}`);
    const marker = await landPageContent(driver, seed, seed.projectId, pageId, session, tag);

    // An uploaded asset plus the same asset pasted under its absolute URL
    // (the only picture form whose raster the renderer embeds).
    await driver.pagesBlocksUploadImage({ name: "scope.png", mime: "image/png", bytes: PIXEL_PNG });
    await expect
      .poll(async () => (await driver.pagesBlocksInlineImages()).length, { timeout: 60_000 })
      .toBeGreaterThan(0);
    const relative = (await driver.pagesBlocksInlineImages())[0]?.src ?? "";
    expect(relative).toContain("/api/assets/v2");
    await pasteImage(driver, `${new URL(driver.page.url()).origin}${relative}`);
    await driver.page.waitForTimeout(3_000);

    await reopenExportDialog(driver);
    const full = pdfFacts(await driver.pageExportConfirmFull());
    await expect.poll(() => driver.pagesHeaderExportDialogVisible(), { timeout: 30_000 }).toBe(false);
    expect(full.imageCount).toBeGreaterThan(0);
    expect(full.text).toContain(marker);

    await reopenExportDialog(driver);
    await driver.pagesHeaderExportSetScope("No images");
    const stripped = pdfFacts(await driver.pageExportConfirmFull());
    await expect.poll(() => driver.pagesHeaderExportDialogVisible(), { timeout: 30_000 }).toBe(false);
    expect(stripped.imageCount).toBe(0);
    expect(stripped.text).toContain(marker);
  }
);

test(
  specTitle(ROW_018, "image-free scope drops markdown image syntax while keeping the text"),
  { tag: specTags(ROW_018) },
  async ({ driver, seed }) => {
    test.setTimeout(600_000);
    const tag = stamp();
    const session = await seedSession(seed);
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Scope018b ${tag}`);
    const path = detailPath(seed, seed.projectId, pageId);
    const shot = `shot018-${tag}`;

    // Land text plus a pasted picture (the editor serializes pictures as
    // markdown image syntax). Re-pinned to intended (NEWFRONT-951; old-app
    // bug NEWFRONT-356): the full scope emits it correctly and the
    // image-free scope drops it.
    let marker = "";
    for (let attempt = 0; attempt < 5; attempt += 1) {
      marker = `mdimg-${tag}-a${attempt}`;
      await driver.openAuthenticated(path, editionBrowserCookies(session, seed));
      await driver.pagesLiveWaitSynced();
      await driver.pagesLiveTypeInBody(`${marker} `);
      await expect.poll(() => driver.pagesLiveBodyText(), { timeout: 60_000 }).toContain(marker);
      await pasteImage(driver, `https://example.com/${shot}.png`);
      await driver.page.waitForTimeout(2_000);
      await driver.pagesHeaderOpenOptions();
      await driver.pagesHeaderChooseMenuItem("Export");
      await driver.pagesHeaderExportSetFormat("Markdown");
      const file = await driver.pageExportConfirmFull();
      await expect.poll(() => driver.pagesHeaderExportDialogVisible(), { timeout: 30_000 }).toBe(false);
      if (file.content.includes(marker) && file.content.includes(shot)) break;
      if (attempt === 4) throw new Error(`[parity] markdown image never landed for ${tag} (NEWFRONT-360).`);
    }

    await reopenExportDialog(driver);
    await driver.pagesHeaderExportSetFormat("Markdown");
    const full = await driver.pageExportConfirmFull();
    await expect.poll(() => driver.pagesHeaderExportDialogVisible(), { timeout: 30_000 }).toBe(false);
    expect(full.content).toContain(marker);
    expect(full.content).toContain(`![](https://example.com/${shot}.png)`);

    // Intended: the image-free scope drops the image line while keeping
    // the text (the image URL appears nowhere else in the document).
    await reopenExportDialog(driver);
    await driver.pagesHeaderExportSetFormat("Markdown");
    await driver.pagesHeaderExportSetScope("No images");
    const stripped = await driver.pageExportConfirmFull();
    await expect.poll(() => driver.pagesHeaderExportDialogVisible(), { timeout: 30_000 }).toBe(false);
    expect(stripped.content).toContain(marker);
    expect(stripped.content).not.toContain(shot);
  }
);

test(
  specTitle(ROW_018, "full-fidelity scope embeds uploaded pictures while image-free drops them"),
  { tag: specTags(ROW_018) },
  async ({ driver, seed }) => {
    test.setTimeout(600_000);
    const tag = stamp();
    const session = await seedSession(seed);
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Scope018c ${tag}`);
    const marker = await landPageContent(driver, seed, seed.projectId, pageId, session, tag);

    // Upload only: the editor holds the asset under its relative stack URL.
    await driver.pagesBlocksUploadImage({ name: "relative.png", mime: "image/png", bytes: PIXEL_PNG });
    await expect
      .poll(async () => (await driver.pagesBlocksInlineImages()).length, { timeout: 60_000 })
      .toBeGreaterThan(0);

    await reopenExportDialog(driver);
    const fullFile = await driver.pageExportConfirmFull();
    await expect.poll(() => driver.pagesHeaderExportDialogVisible(), { timeout: 30_000 }).toBe(false);
    const full = pdfFacts(fullFile);
    expect(full.text).toContain(marker);

    await reopenExportDialog(driver);
    await driver.pagesHeaderExportSetScope("No images");
    const strippedFile = await driver.pageExportConfirmFull();
    await expect.poll(() => driver.pagesHeaderExportDialogVisible(), { timeout: 30_000 }).toBe(false);
    const stripped = pdfFacts(strippedFile);
    expect(stripped.text).toContain(marker);

    // Re-pinned to intended (NEWFRONT-951; old-app bug NEWFRONT-470):
    // the full-fidelity scope embeds the uploaded raster, so the full
    // file is larger and carries the raster's image objects while the
    // image-free scope drops them. One RGBA raster embeds as two image
    // objects (image plus soft mask): a paste-only probe of the same
    // bytes through the same pipeline reads imageCount 2, so the
    // re-pinned count is 2, not the 1 the issue text guessed (see
    // NEWFRONT-951; NEWFRONT-952 routes uploads through this pipeline).
    expect(fullFile.byteLength).toBeGreaterThan(strippedFile.byteLength);
    expect(full.imageCount).toBe(2);
    expect(stripped.imageCount).toBe(0);
  }
);

test(
  specTitle(ROW_019, "paginated document honors the paper size, typeface, title and file name"),
  { tag: specTags(ROW_019) },
  async ({ driver, seed }) => {
    test.setTimeout(600_000);
    const tag = stamp();
    const session = await seedSession(seed);
    const title = `Doc019 ${tag}`;
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, title);
    const marker = await landPageContent(driver, seed, seed.projectId, pageId, session, tag);
    const slug = pageTitleSlug(title);
    const traffic = captureApiPaths(driver);

    await test.step("the default size renders an A4 document in the product typeface", async () => {
      await reopenExportDialog(driver);
      const file = await driver.pageExportConfirmFull();
      await expect.poll(() => driver.pagesHeaderExportDialogVisible(), { timeout: 30_000 }).toBe(false);
      expect(file.fileName).toBe(`${slug}-a4.pdf`);
      expect(file.content.startsWith("%PDF")).toBe(true);
      const facts = pdfFacts(file);
      expect(facts.mediaBoxes.length).toBeGreaterThan(0);
      for (const box of facts.mediaBoxes) expect(box).toBe("0 0 595.280029 841.890015");
      expect(facts.baseFonts.length).toBeGreaterThan(0);
      for (const font of facts.baseFonts) expect(font).toMatch(/Inter/i);
      expect(facts.text).toContain(title);
      expect(facts.text).toContain(marker);
    });

    await test.step("letter renders at letter size with the size in the file name", async () => {
      await reopenExportDialog(driver);
      await driver.pagesHeaderExportSetPageFormat("Letter");
      const file = await driver.pageExportConfirmFull();
      await expect.poll(() => driver.pagesHeaderExportDialogVisible(), { timeout: 30_000 }).toBe(false);
      expect(file.fileName).toBe(`${slug}-letter.pdf`);
      const facts = pdfFacts(file);
      expect(facts.mediaBoxes.length).toBeGreaterThan(0);
      for (const box of facts.mediaBoxes) expect(box).toBe("0 0 612 792");
      expect(facts.text).toContain(title);
      expect(facts.text).toContain(marker);
    });

    // The render is fully client-side: no export-shaped API traffic ran.
    expect(traffic.stop().filter((path) => /export/i.test(path))).toEqual([]);
  }
);

test(
  specTitle(ROW_020, "plain-text export converts the body honoring the scope, named from the title"),
  { tag: specTags(ROW_020) },
  async ({ driver, seed }) => {
    test.setTimeout(600_000);
    const tag = stamp();
    const session = await seedSession(seed);
    const title = `Text020 ${tag}`;
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, title);
    const marker = await landPageContent(driver, seed, seed.projectId, pageId, session, tag);
    const slug = pageTitleSlug(title);
    const traffic = captureApiPaths(driver);

    await reopenExportDialog(driver);
    await driver.pagesHeaderExportSetFormat("Markdown");
    const full = await driver.pageExportConfirmFull();
    await expect.poll(() => driver.pagesHeaderExportDialogVisible(), { timeout: 30_000 }).toBe(false);
    expect(full.fileName).toBe(`${slug}.md`);
    expect(full.content).toContain(marker);

    await reopenExportDialog(driver);
    await driver.pagesHeaderExportSetFormat("Markdown");
    await driver.pagesHeaderExportSetScope("No images");
    const stripped = await driver.pageExportConfirmFull();
    await expect.poll(() => driver.pagesHeaderExportDialogVisible(), { timeout: 30_000 }).toBe(false);
    expect(stripped.fileName).toBe(`${slug}.md`);
    expect(stripped.content).toContain(marker);

    // The conversion is fully client-side: no export-shaped API traffic ran.
    expect(traffic.stop().filter((path) => /export/i.test(path))).toEqual([]);
  }
);

test(
  specTitle(ROW_021, "success closes with a confirmation and resets; cancelling discards without exporting"),
  { tag: specTags(ROW_021) },
  async ({ driver, seed }) => {
    test.setTimeout(600_000);
    const tag = stamp();
    const session = await seedSession(seed);
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Feed021 ${tag}`);
    await driver.openAuthenticated(detailPath(seed, seed.projectId, pageId), editionBrowserCookies(session, seed));
    await driver.pagesLiveWaitSynced();
    await driver.pagesLiveTypeInBody(`feed-${tag} `);

    await test.step("success confirms, closes, and resets the form", async () => {
      await reopenExportDialog(driver);
      await driver.pagesHeaderExportSetFormat("Markdown");
      await driver.pagesHeaderExportSetScope("No images");
      await driver.pageExportConfirmFull();
      await waitForToast(driver, "Page exported successfully.");
      await expect.poll(() => driver.pagesHeaderExportDialogVisible(), { timeout: 30_000 }).toBe(false);
      await reopenExportDialog(driver);
      expect(await driver.pagesHeaderExportForm()).toEqual({
        format: "PDF",
        scope: "Everything",
        pageFormat: "A4",
        pageFormatVisible: true,
      });
      await driver.pagesHeaderExportCancel();
    });

    await test.step("cancelling downloads nothing and discards the choices", async () => {
      await reopenExportDialog(driver);
      await driver.pagesHeaderExportSetFormat("Markdown");
      await driver.pagesHeaderExportSetScope("No images");
      const noDownload = driver.page.waitForEvent("download", { timeout: 3_000 }).then(
        () => false,
        () => true
      );
      await driver.pagesHeaderExportCancel();
      await expect.poll(() => driver.pagesHeaderExportDialogVisible(), { timeout: 15_000 }).toBe(false);
      expect(await noDownload).toBe(true);
      await reopenExportDialog(driver);
      expect(await driver.pagesHeaderExportForm()).toEqual({
        format: "PDF",
        scope: "Everything",
        pageFormat: "A4",
        pageFormatVisible: true,
      });
      await driver.pagesHeaderExportCancel();
    });
  }
);

test(
  specTitle(ROW_021, "failure keeps the dialog open with an error and the choices intact"),
  { tag: specTags(ROW_021) },
  async ({ driver, seed }) => {
    test.setTimeout(600_000);
    const tag = stamp();
    const session = await seedSession(seed);
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Feed021b ${tag}`);
    await driver.openAuthenticated(detailPath(seed, seed.projectId, pageId), editionBrowserCookies(session, seed));
    await driver.pagesLiveWaitSynced();
    await driver.pagesLiveTypeInBody(`feedb-${tag} `);

    await reopenExportDialog(driver);
    await driver.pagesHeaderExportSetFormat("Markdown");
    await driver.pagesHeaderExportSetScope("No images");
    await driver.pageExportFailSave();

    // The confirm click races the no-download proof: the save throws
    // before any download event can land.
    const dialog = driver.page.getByRole("dialog").filter({ hasText: "Export page" }).last();
    const noDownload = driver.page.waitForEvent("download", { timeout: 5_000 }).then(
      () => false,
      () => true
    );
    await dialog.getByRole("button", { name: "Export", exact: true }).click({ timeout: 30_000 });
    await waitForToast(driver, "Page could not be exported.", 15_000);
    expect(await noDownload).toBe(true);
    expect(await driver.pagesHeaderExportDialogVisible()).toBe(true);
    expect(await driver.pagesHeaderExportForm()).toEqual({
      format: "Markdown",
      scope: "No images",
      pageFormat: "",
      pageFormatVisible: false,
    });
    // The confirm action returns to its idle label (no stuck working state).
    expect(await dialog.getByRole("button", { name: "Export", exact: true }).count()).toBe(1);

    // Recovery: a reload clears the sabotage and the export succeeds.
    await driver.page.reload();
    await driver.pagesLiveWaitSynced();
    await reopenExportDialog(driver);
    await driver.pagesHeaderExportSetFormat("Markdown");
    await driver.pageExportConfirmFull();
    await waitForToast(driver, "Page exported successfully.");
    await expect.poll(() => driver.pagesHeaderExportDialogVisible(), { timeout: 30_000 }).toBe(false);
  }
);

interface CrossSurfaceWorld {
  owner: AuthedSession;
  workspaceSlug: string;
  projectId: string;
}

/** Fresh owner plus workspace with one project and one work item (analytics/profile/settings need data). */
async function setupCrossSurfaceWorld(prefix: string): Promise<CrossSurfaceWorld> {
  const owner = await signUpAuthedSession(prefix);
  const suffix = uniqueSuffixForProjects();
  const workspaceName = `Exp WS ${suffix}`;
  const ws = await createWorkspaceForProjects(owner, { name: workspaceName, slug: `exp-${suffix}` });
  await markOnboardedForProjects(owner);
  await setLastWorkspaceForProjects(owner, ws.id);
  await drainWorkspaceSeed(owner, ws.slug, workspaceName);
  const projectId = await createProjectViaApi(owner, ws.slug, {
    name: `Exp Proj ${suffix}`,
    identifier: parityProjectIdentifier("EQ"),
  });
  await createIssue(ws.slug, projectId, owner.cookie, `Exp issue ${suffix}`);
  return { owner, workspaceSlug: ws.slug, projectId };
}

/** Tab from the top until the focused label matches, or null past the bound. */
async function tabToFocusedLabel(
  driver: ParityDriver,
  match: (label: string) => boolean,
  bound = 80
): Promise<string | null> {
  await driver.page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
  for (let tab = 0; tab < bound; tab += 1) {
    await driver.page.keyboard.press("Tab");
    const facts = await driver.pageExportFocusedFacts();
    if (facts !== null && match(facts.label)) return facts.label;
  }
  return null;
}

/**
 * tabIndex of the first control whose accessible name contains `sub` (or
 * equals it when `exact`), or null when absent. Selects are included:
 * the new app renders the dialog rows as native selects, matched by
 * their option text.
 */
async function controlTabIndex(
  driver: ParityDriver,
  scope: "page" | "dialog",
  sub: string,
  exact = false
): Promise<number | null> {
  return driver.page.evaluate(
    ({ scope: which, needle, exact: isExact }) => {
      const root =
        which === "dialog"
          ? ([...document.querySelectorAll("[role='dialog']")].at(-1) ?? document)
          : (document.querySelector("main") ?? document);
      const hit = [...root.querySelectorAll("button, a, input, select, [role='button']")].find((el) => {
        const name = ((el as HTMLElement).getAttribute("aria-label") ?? el.textContent ?? "")
          .trim()
          .replace(/\s+/g, " ")
          .toLowerCase();
        return isExact ? name === needle.toLowerCase() : name.includes(needle.toLowerCase());
      }) as HTMLElement | undefined;
      return hit ? hit.tabIndex : null;
    },
    { scope, needle: sub, exact }
  );
}

/**
 * Whether focusing the control changes any rendered style (background,
 * border, outline-style, shadow). The outline *width* is ignored: it flips
 * under `outline-style: none`, which renders nothing either way.
 */
async function focusStyleUnchanged(
  driver: ParityDriver,
  scope: "page" | "dialog",
  sub: string,
  exact = false
): Promise<boolean> {
  return driver.page.evaluate(
    ({ scope: which, needle, exact: isExact }) => {
      const root =
        which === "dialog"
          ? ([...document.querySelectorAll("[role='dialog']")].at(-1) ?? document)
          : (document.querySelector("main") ?? document);
      const hit = [...root.querySelectorAll("button, a, input, select, [role='button']")].find((el) => {
        const name = ((el as HTMLElement).getAttribute("aria-label") ?? el.textContent ?? "")
          .trim()
          .replace(/\s+/g, " ")
          .toLowerCase();
        return isExact ? name === needle.toLowerCase() : name.includes(needle.toLowerCase());
      }) as HTMLElement | undefined;
      if (!hit) return false;
      const snap = (): string => {
        const style = getComputedStyle(hit);
        return JSON.stringify({
          bg: style.backgroundColor,
          border: style.borderColor,
          outline: style.outlineStyle,
          box: style.boxShadow,
        });
      };
      (document.activeElement as HTMLElement | null)?.blur();
      hit.blur();
      const blurred = snap();
      hit.focus();
      return snap() === blurred;
    },
    { scope, needle: sub, exact }
  );
}

/** Focus the control by name (exact when `exact`); resolves false when absent. */
async function focusControlByName(
  driver: ParityDriver,
  scope: "page" | "dialog",
  sub: string,
  exact = false
): Promise<boolean> {
  return driver.page.evaluate(
    ({ scope: which, needle, exact: isExact }) => {
      const root =
        which === "dialog"
          ? ([...document.querySelectorAll("[role='dialog']")].at(-1) ?? document)
          : (document.querySelector("main") ?? document);
      const hit = [...root.querySelectorAll("button, a, input, select, [role='button']")].find((el) => {
        const name = ((el as HTMLElement).getAttribute("aria-label") ?? el.textContent ?? "")
          .trim()
          .replace(/\s+/g, " ")
          .toLowerCase();
        return isExact ? name === needle.toLowerCase() : name.includes(needle.toLowerCase());
      }) as HTMLElement | undefined;
      if (!hit) return false;
      hit.focus();
      return document.activeElement === hit;
    },
    { scope, needle: sub, exact }
  );
}

/** Press content keys and prove nothing happens: same URL, no dialog, no notice, no download. */
async function expectKeysInert(driver: ParityDriver): Promise<void> {
  // Quiesce first: the previous step's confirmation may still show.
  await expect.poll(() => driver.rulesLastToast(), { timeout: 30_000 }).toBeNull();
  const url = driver.page.url();
  const noDownload = driver.page.waitForEvent("download", { timeout: 2_500 }).then(
    () => false,
    () => true
  );
  for (const key of ["e", "x", "p", "m", "?"]) {
    await driver.page.keyboard.press(key);
    await driver.page.waitForTimeout(150);
  }
  expect(driver.page.url()).toBe(url);
  expect(await driver.page.getByRole("dialog").count()).toBe(0);
  expect(await driver.rulesLastToast()).toBeNull();
  expect(await noDownload).toBe(true);
}

test(
  specTitle(ROW_023, "export controls operate by keyboard and no screen defines custom bindings"),
  { tag: specTags(ROW_023) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const world = await setupCrossSurfaceWorld("parity-exp023");

    await test.step("the settings submit fires from the keyboard", async () => {
      await driver.exportsSettingsOpen(world.workspaceSlug, browserSessionCookies(world.owner));
      await expect.poll(() => driver.exportsSettingsAccess(), { timeout: 60_000 }).toBe("form");
      expect(await tabToFocusedLabel(driver, (label) => label === "button :: Export")).toBe("button :: Export");
      await driver.page.keyboard.press("Enter");
      await expect.poll(() => driver.exportsSettingsHistoryCount(), { timeout: 120_000 }).toBeGreaterThan(0);
      await expectKeysInert(driver);
    });

    await test.step("an analytics export action saves its file from the keyboard", async () => {
      await driver.analyticsExportOpen(world.workspaceSlug, browserSessionCookies(world.owner));
      await expect.poll(() => driver.analyticsExportButtonCount(), { timeout: 120_000 }).toBe(2);
      expect(await tabToFocusedLabel(driver, (label) => label.includes("Export as csv"))).not.toBeNull();
      const [download] = await Promise.all([
        driver.page.waitForEvent("download", { timeout: 60_000 }),
        driver.page.keyboard.press("Enter"),
      ]);
      expect(download.suggestedFilename()).toBe(`${world.workspaceSlug}-analytics.csv`);
      await expectKeysInert(driver);
    });

    await test.step("the profile download saves its file from the keyboard", async () => {
      await driver.profileActivityOpen(world.workspaceSlug, world.owner.userId, browserSessionCookies(world.owner));
      await expect.poll(() => driver.profileActivityDownloadVisible(), { timeout: 60_000 }).toBe(true);
      expect(await tabToFocusedLabel(driver, (label) => label.includes("Download"))).not.toBeNull();
      const [download] = await Promise.all([
        driver.page.waitForEvent("download", { timeout: 60_000 }),
        driver.page.keyboard.press("Enter"),
      ]);
      expect(download.suggestedFilename()).toMatch(/^profile-activity-\d+\.csv$/);
      await expectKeysInert(driver);
    });

    await test.step("the dialog confirms and dismisses from the keyboard", async () => {
      const tag = stamp();
      const session = await seedSession(seed);
      const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Kbd023 ${tag}`);
      await openSeedPageAfterWorld(driver, detailPath(seed, seed.projectId, pageId), session, seed);
      await driver.pagesLiveWaitSynced();
      await driver.pagesHeaderOpenOptions();
      await driver.pagesHeaderChooseMenuItem("Export");
      await expect.poll(() => driver.pagesHeaderExportDialogVisible(), { timeout: 15_000 }).toBe(true);

      // Tab cycles the footer actions; Enter on Export downloads and closes.
      expect(await tabToFocusedLabel(driver, (label) => label === "button :: Cancel")).toBe("button :: Cancel");
      expect(await tabToFocusedLabel(driver, (label) => label === "button :: Export")).toBe("button :: Export");
      const [download] = await Promise.all([
        driver.page.waitForEvent("download", { timeout: 60_000 }),
        driver.page.keyboard.press("Enter"),
      ]);
      expect(download.suggestedFilename()).toMatch(/\.pdf$/);
      await waitForToast(driver, "Page exported successfully.");
      await expect.poll(() => driver.pagesHeaderExportDialogVisible(), { timeout: 30_000 }).toBe(false);

      // Escape dismisses; content keys with the dialog open change nothing.
      await reopenExportDialog(driver);
      await expect.poll(() => driver.rulesLastToast(), { timeout: 30_000 }).toBeNull();
      const formBefore = JSON.stringify(await driver.pagesHeaderExportForm());
      for (const key of ["e", "x", "p", "m", "?"]) {
        await driver.page.keyboard.press(key);
        await driver.page.waitForTimeout(150);
      }
      expect(JSON.stringify(await driver.pagesHeaderExportForm())).toBe(formBefore);
      expect(await driver.rulesLastToast()).toBeNull();
      await driver.page.keyboard.press("Escape");
      await expect.poll(() => driver.pagesHeaderExportDialogVisible(), { timeout: 15_000 }).toBe(false);
    });
  }
);

test(
  specTitle(ROW_023, "every control is Tab-reachable with a visible focus indicator and keyboard-navigable menus"),
  { tag: specTags(ROW_023) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const world = await setupCrossSurfaceWorld("parity-exp023b");

    // Re-pinned to intended (NEWFRONT-951; old-app bug NEWFRONT-471 left
    // every picker at tabIndex -1, menus ignoring arrows/Enter, and no
    // control showing a focus change).
    await test.step("settings pickers are Tab-reachable", async () => {
      await driver.exportsSettingsOpen(world.workspaceSlug, browserSessionCookies(world.owner));
      await expect.poll(() => driver.exportsSettingsAccess(), { timeout: 60_000 }).toBe("form");
      // Intended: every control tab-reachable. The guide mounts
      // asynchronously behind the settings frame, so the first read
      // polls until it lands.
      await expect.poll(() => controlTabIndex(driver, "page", "all projects"), { timeout: 60_000 }).toBe(0);
      expect(await controlTabIndex(driver, "page", "csv")).toBe(0);
      expect(await controlTabIndex(driver, "page", "refresh status")).toBe(0);
    });

    await test.step("settings, analytics and profile controls show a focus change", async () => {
      // Intended: a visible focus indicator. A Tab first establishes
      // keyboard modality so :focus-visible outlines render; the tabIndex
      // reads alongside prove the control exists (an absent control
      // returns false, which would pass the style read alone).
      await driver.page.keyboard.press("Tab");
      expect(await controlTabIndex(driver, "page", "export", true)).toBe(0);
      expect(await focusStyleUnchanged(driver, "page", "export", true)).toBe(false);
      expect(await controlTabIndex(driver, "page", "refresh status")).toBe(0);
      expect(await focusStyleUnchanged(driver, "page", "refresh status")).toBe(false);
      await driver.analyticsExportOpen(world.workspaceSlug, browserSessionCookies(world.owner));
      await expect.poll(() => driver.analyticsExportButtonCount(), { timeout: 120_000 }).toBe(2);
      await driver.page.keyboard.press("Tab");
      expect(await controlTabIndex(driver, "page", "export as csv")).toBe(0);
      expect(await focusStyleUnchanged(driver, "page", "export as csv")).toBe(false);
      await driver.profileActivityOpen(world.workspaceSlug, world.owner.userId, browserSessionCookies(world.owner));
      await expect.poll(() => driver.profileActivityDownloadVisible(), { timeout: 60_000 }).toBe(true);
      await driver.page.keyboard.press("Tab");
      expect(await controlTabIndex(driver, "page", "download")).toBe(0);
      expect(await focusStyleUnchanged(driver, "page", "download")).toBe(false);
    });

    await test.step("dialog rows are Tab-reachable and the footer shows a focus change", async () => {
      const tag = stamp();
      const session = await seedSession(seed);
      const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Kbd023b ${tag}`);
      await openSeedPageAfterWorld(driver, detailPath(seed, seed.projectId, pageId), session, seed);
      await driver.pagesLiveWaitSynced();
      await driver.pagesHeaderOpenOptions();
      await driver.pagesHeaderChooseMenuItem("Export");
      await expect.poll(() => driver.pagesHeaderExportDialogVisible(), { timeout: 15_000 }).toBe(true);

      // The row selects show their current values, so match on those.
      expect(await controlTabIndex(driver, "dialog", "pdf")).toBe(0);
      expect(await controlTabIndex(driver, "dialog", "everything")).toBe(0);
      expect(await controlTabIndex(driver, "dialog", "a4")).toBe(0);
      await driver.page.keyboard.press("Tab");
      expect(await controlTabIndex(driver, "dialog", "cancel")).toBe(0);
      expect(await controlTabIndex(driver, "dialog", "export")).toBe(0);
      expect(await focusStyleUnchanged(driver, "dialog", "cancel")).toBe(false);
      expect(await focusStyleUnchanged(driver, "dialog", "export")).toBe(false);

      // After a Tab establishes keyboard modality, focusing the format
      // select draws its visible indicator.
      expect(await focusControlByName(driver, "dialog", "pdf")).toBe(true);
      const focused = await driver.pageExportFocusedFacts();
      expect(focused?.indicator).toBe("outline");
      expect(focused?.focusVisible).toBe(true);
    });

    await test.step("dialog picker menus navigate and select by keyboard", async () => {
      // The dialog is still open from the previous step.
      await expect.poll(() => driver.pagesHeaderExportDialogVisible(), { timeout: 15_000 }).toBe(true);
      expect(await focusControlByName(driver, "dialog", "pdf")).toBe(true);
      expect((await driver.pagesHeaderExportForm()).format).toBe("PDF");
      expect(await driver.pageExportOfferedOptions("format")).toEqual(["PDF", "Markdown"]);
      // A focused native select moves its selection with arrows.
      await driver.page.keyboard.press("ArrowDown");
      await expect
        .poll(async () => (await driver.pagesHeaderExportForm()).format, { timeout: 15_000 })
        .toBe("Markdown");
      await driver.page.keyboard.press("ArrowUp");
      await expect.poll(async () => (await driver.pagesHeaderExportForm()).format, { timeout: 15_000 }).toBe("PDF");
    });
  }
);

test(
  specTitle(ROW_024, "export behavior is identical across editions and desktop"),
  { tag: specTags(ROW_024) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    // Method (recorded in the row): the scratch stack serves one build, so
    // the scenario pins the observable half — every export surface renders
    // ungated, entry talks only to the known endpoints with no
    // edition/desktop markers in traffic or DOM — while the inventory
    // records the source sweep (no edition-gated or desktop-only export
    // code anywhere). Any gate, stub, or forked endpoint fails the render,
    // the marker scan, or the allowlist.
    const world = await setupCrossSurfaceWorld("parity-exp024");
    const tag = stamp();
    const session = await seedSession(seed);
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Edn024 ${tag}`);
    const traffic = captureApiPaths(driver);

    await test.step("the settings surface renders ungated with no markers", async () => {
      await driver.exportsSettingsOpen(world.workspaceSlug, browserSessionCookies(world.owner));
      await expect.poll(() => driver.exportsSettingsAccess(), { timeout: 60_000 }).toBe("form");
      expect(await driver.pageExportGateMarkersPresent()).toBe(false);
    });

    await test.step("the analytics surface renders ungated with no markers", async () => {
      await driver.analyticsExportOpen(world.workspaceSlug, browserSessionCookies(world.owner));
      await expect.poll(() => driver.analyticsExportButtonCount(), { timeout: 120_000 }).toBe(2);
      expect(await driver.pageExportGateMarkersPresent()).toBe(false);
    });

    await test.step("the profile surface renders ungated with no markers", async () => {
      await driver.profileActivityOpen(world.workspaceSlug, world.owner.userId, browserSessionCookies(world.owner));
      await expect.poll(() => driver.profileActivityDownloadVisible(), { timeout: 60_000 }).toBe(true);
      expect(await driver.pageExportGateMarkersPresent()).toBe(false);
    });

    await test.step("the page-export dialog renders ungated with no markers", async () => {
      await openSeedPageAfterWorld(driver, detailPath(seed, seed.projectId, pageId), session, seed);
      await driver.pagesLiveWaitSynced();
      await driver.pagesHeaderOpenOptions();
      await driver.pagesHeaderChooseMenuItem("Export");
      await expect.poll(() => driver.pagesHeaderExportDialogVisible(), { timeout: 15_000 }).toBe(true);
      expect(await driver.pageExportHeading()).toBe("Export page");
      expect(await driver.pageExportGateMarkersPresent()).toBe(false);
      await driver.pagesHeaderExportCancel();
    });

    await test.step("entry traffic stays inside the known endpoints with no gate markers", async () => {
      const paths = traffic.stop();
      expect(paths).toContain(`/api/workspaces/${world.workspaceSlug}/export-issues`);
      expect(paths.filter((path) => /export-issues/.test(path)).length).toBeGreaterThan(0);
      // Cloud chrome phones home around every page, on both apps: the old
      // app refreshes the auth scope, the new app reads the plan snapshot
      // for the sidebar badge (CLOUD-038). Both are ambient shell reads,
      // not export entry — observed as the only denylisted paths on each
      // target's cloud run — so the cloud leg sets exactly those two aside
      // before the sweep. Every other billing/license/upgrade-shaped call
      // still fails, on both editions.
      const ambient = isCloudEdition() ? ["/api/auth/scope-upgrade/", "/api/billing/me/plan/"] : [];
      const swept = paths.filter((path) => !ambient.includes(path));
      expect(swept.filter((path) => /desktop|license|billing|premium|upgrade|\bee\b/i.test(path))).toEqual([]);
    });
  }
);
