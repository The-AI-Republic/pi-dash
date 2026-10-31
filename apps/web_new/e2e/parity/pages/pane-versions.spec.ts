// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-317): the pages side navigation pane — the
// slide-over shell with its shareable tab key, the canvas-edge heading
// preview, the outline/assets/info tabs — and the version timeline with
// its snapshot overlay, gated restore, and error handling.
// Rows: PAGE-045–PAGE-054.
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  requireMentionMember,
  serverCleanupProject,
  serverCreatePage,
  serverCreateProjectWithFlags,
  serverEnsureProjectMember,
  serverPageDescription,
  serverPageLock,
  serverPageRecord,
  serverPageVersionDetail,
  serverPageVersions,
  serverUpdatePageDescription,
  signInSession,
  type ParityPageVersion,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";

/** Unique area tag so scratch projects and page names never collide. */
function tag(area: string): string {
  return `NF317 ${area} ${Date.now()}`;
}

/** Scratch project for one scenario (deleted with the project in `finally`). */
async function scratchProject(
  seed: ParitySeedFacts,
  area: string,
  session: string
): Promise<{ projectId: string; name: string }> {
  const name = `${tag(area)} project`;
  const projectId = await serverCreateProjectWithFlags(
    seed.workspaceSlug,
    name,
    parityProjectIdentifier("N317"),
    {},
    session
  );
  return { projectId, name };
}

/** Minimal TipTap document with one paragraph per text. */
function tiptapParagraphs(...texts: string[]): string {
  return JSON.stringify({
    type: "doc",
    content: texts.map((text) => ({
      type: "paragraph",
      content: text === "" ? [] : [{ type: "text", text }],
    })),
  });
}

/** Matching HTML with one paragraph per text. */
function htmlParagraphs(...texts: string[]): string {
  return texts.map((text) => `<p>${text}</p>`).join("");
}

/**
 * Poll the version list until `count` snapshots persist (creation runs in
 * a background worker, so fixtures never assume it is synchronous).
 */
async function waitVersions(
  seed: ParitySeedFacts,
  projectId: string,
  pageId: string,
  session: string,
  count: number,
  timeoutMs = 180_000
): Promise<ParityPageVersion[]> {
  const started = Date.now();
  for (;;) {
    const rows = await serverPageVersions(seed.workspaceSlug, projectId, pageId, session);
    if (rows.length >= count) return rows;
    if (Date.now() - started > timeoutMs) {
      throw new Error(`[parity] versions stuck at ${rows.length}, wanted ${count}.`);
    }
    await new Promise((resolve) => setTimeout(resolve, 3_000));
  }
}

/**
 * Poll one snapshot's detail until its payloads carry the fixture (the
 * persisting worker runs async, so fixtures never assume it is done).
 */
async function waitVersionDetail(
  seed: ParitySeedFacts,
  projectId: string,
  pageId: string,
  versionId: string,
  session: string,
  want: { html?: string; json?: string },
  timeoutMs = 180_000
): Promise<void> {
  const started = Date.now();
  for (;;) {
    const detail = await serverPageVersionDetail(seed.workspaceSlug, projectId, pageId, versionId, session);
    const htmlOk = want.html === undefined || detail.html.includes(want.html);
    const jsonOk = want.json === undefined || (detail.jsonText ?? "").includes(want.json);
    if (htmlOk && jsonOk) return;
    if (Date.now() - started > timeoutMs) {
      throw new Error(`[parity] version ${versionId} detail never carried the fixture.`);
    }
    await new Promise((resolve) => setTimeout(resolve, 3_000));
  }
}

/** Poll the live record until its HTML contains `text` (autosave). */
async function waitDescriptionContains(
  seed: ParitySeedFacts,
  projectId: string,
  pageId: string,
  session: string,
  text: string,
  timeoutMs = 120_000
): Promise<void> {
  const started = Date.now();
  for (;;) {
    const record = await serverPageRecord(seed.workspaceSlug, projectId, pageId, session);
    if (record.descriptionHtml.includes(text)) return;
    if (Date.now() - started > timeoutMs) {
      throw new Error(`[parity] description never contained ${JSON.stringify(text)}.`);
    }
    await new Promise((resolve) => setTimeout(resolve, 3_000));
  }
}

/**
 * Settle the pane's shared editor handle after a cold open. The handle
 * the pane reads can stick to a pre-settle instance and stay deaf to
 * further edits; leaving through the creator profile link and coming
 * back remounts the detail view on warm client state so it hears them.
 * Liveness is verified, not assumed: a typed character must move the
 * word count before the setup passes, otherwise the roundtrip repeats.
 * Ends with the pane closed and the probe character cleared.
 */
async function convergeLivePane(
  driver: ParityDriver,
  seed: ParitySeedFacts,
  projectId: string,
  pageId: string
): Promise<void> {
  let lastError: unknown = null;
  for (let attempt = 0; attempt < 4; attempt++) {
    try {
      // Retries reopen from scratch: a failed attempt can strand the view
      // mid-roundtrip, and every attempt must start from a known state.
      if (attempt > 0) await driver.pagesPaneOpenDetail(seed.workspaceSlug, projectId, pageId);
      if (await driver.pagesPaneIsOpen()) await driver.pagesPaneClose();
      await driver.pagesPaneOpenViaToolbar();
      await driver.pagesPaneSelectTab("Info");
      await driver.pagesPaneProfileRoundtrip();
      if (!(await driver.pagesPaneIsOpen())) {
        await driver.pagesPaneOpenViaToolbar();
        await driver.pagesPaneSelectTab("Info");
      }
      await driver.pagesPaneEditorClickEnd();
      const wordsBefore = (await driver.pagesPaneStats()).words;
      await driver.pagesPaneEditorType("z");
      await expect.poll(async () => (await driver.pagesPaneStats()).words, { timeout: 15_000 }).not.toBe(wordsBefore);
      try {
        await driver.pagesPaneEditorClear();
        await expect.poll(async () => (await driver.pagesPaneStats()).words, { timeout: 15_000 }).toBe(0);
      } catch {
        // Best effort: a leftover character is a plain paragraph, never a
        // heading, so it cannot disturb the bars, outline, or counts below.
      }
      await driver.pagesPaneClose();
      return;
    } catch (error) {
      lastError = error;
      const line = error instanceof Error ? error.message.split("\n")[0] : String(error);
      console.log(`[parity] convergeLivePane attempt ${attempt + 1} failed, retrying (${line}).`);
    }
  }
  const detail = lastError instanceof Error ? lastError.message.split("\n")[0] : String(lastError);
  throw new Error(`[parity] the pane never heard a live edit (${detail}).`);
}

/** Most recent toast as one string, polled until non-empty. */
async function toastText(driver: ParityDriver): Promise<string> {
  let text = "";
  await expect
    .poll(
      async () => {
        const toast = await driver.rulesLastToast();
        text = toast ? `${toast.title} ${toast.message}` : "";
        return text;
      },
      { timeout: 15_000 }
    )
    .not.toBe("");
  return text;
}

test(
  specTitle(["PAGE-045"], "pane shell slides at a fixed width with ordered shareable tabs and reflows the canvas"),
  { tag: specTags(["PAGE-045"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "shell", session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("shell")} page`);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("the pane starts closed with the preview at the canvas edge", async () => {
        await driver.pagesPaneOpenDetail(seed.workspaceSlug, projectId, pageId);
        expect(await driver.pagesPaneIsOpen()).toBe(false);
        expect(await driver.pagesPaneQueryParam("paneTab")).toBe(null);
        await convergeLivePane(driver, seed, projectId, pageId);
        await driver.pagesPaneEditorPasteHtml("<h1>Shell head</h1><p>Shell body</p>");
        await expect.poll(() => driver.pagesPanePreviewVisible(), { timeout: 30_000 }).toBe(true);
      });

      await test.step("the fixture saves settle before the shell moves", async () => {
        await waitDescriptionContains(seed, projectId, pageId, session, "Shell body");
        expect(await waitVersions(seed, projectId, pageId, session, 1)).toHaveLength(1);
      });

      const canvasClosed = await driver.pagesPaneCanvasWidth();
      await test.step("opening slides in a fixed-width pane with ordered tabs", async () => {
        await driver.pagesPaneOpenViaToolbar();
        expect(await driver.pagesPaneIsOpen()).toBe(true);
        expect(await driver.pagesPaneQueryParam("paneTab")).toBe("outline");
        expect(await driver.pagesPaneWidth()).toBeGreaterThan(290);
        expect(await driver.pagesPaneWidth()).toBeLessThan(298);
        expect(await driver.pagesPaneTabNames()).toEqual(["Outline", "Info", "Assets"]);
        expect(await driver.pagesPaneSlideTransition()).toBe(true);
      });

      await test.step("opening and closing reflows the canvas", async () => {
        // The shell slides (PAGE-045 pins the transition), so a width read
        // right after open/close can land mid-flight under load; poll for
        // the settled layout instead of asserting the first frame.
        await expect
          .poll(async () => canvasClosed - (await driver.pagesPaneCanvasWidth()), { timeout: 30_000 })
          .toBeGreaterThan(270);
        const canvasOpen = await driver.pagesPaneCanvasWidth();
        expect(canvasClosed - canvasOpen).toBeLessThan(318);
        await driver.pagesPaneClose();
        expect(await driver.pagesPaneIsOpen()).toBe(false);
        expect(await driver.pagesPaneQueryParam("paneTab")).toBe(null);
        await expect
          .poll(async () => Math.abs((await driver.pagesPaneCanvasWidth()) - canvasClosed), { timeout: 30_000 })
          .toBeLessThan(24);
      });

      await test.step("the active tab is shareable by URL", async () => {
        await driver.pagesPaneOpenDetailQuery(seed.workspaceSlug, projectId, pageId, "paneTab=info");
        expect(await driver.pagesPaneIsOpen()).toBe(true);
        expect(await driver.pagesPaneActiveTab()).toBe("info");
        // The info panel renders for the deep link (statistics cards read).
        expect((await driver.pagesPaneStats()).words).toBeGreaterThanOrEqual(0);
        await driver.pagesPaneSelectTab("Assets");
        expect(await driver.pagesPaneQueryParam("paneTab")).toBe("assets");
        await driver.pagesPaneSelectTab("Outline");
        expect(await driver.pagesPaneQueryParam("paneTab")).toBe("outline");
      });

      await test.step("an unknown tab key keeps the pane shut (no extension registry)", async () => {
        await driver.pagesPaneOpenDetailQuery(seed.workspaceSlug, projectId, pageId, "paneTab=zzz");
        expect(await driver.pagesPaneIsOpen()).toBe(false);
        expect(await driver.pagesPaneTabNames()).toEqual([]);
        expect(Math.abs((await driver.pagesPaneCanvasWidth()) - canvasClosed)).toBeLessThan(24);
      });

      await test.step("the shell moves write no server state", async () => {
        expect(await serverPageVersions(seed.workspaceSlug, projectId, pageId, session)).toHaveLength(1);
        const record = await serverPageRecord(seed.workspaceSlug, projectId, pageId, session);
        expect(record.id).toBe(pageId);
        expect(record.descriptionHtml).toContain("Shell body");
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-046"], "heading preview floats at the canvas edge, expands on hover, and updates live"),
  { tag: specTags(["PAGE-046"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "preview", session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("preview")} page`);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.pagesPaneOpenDetail(seed.workspaceSlug, projectId, pageId);

      await test.step("one bar renders per heading while the pane is closed", async () => {
        expect(await driver.pagesPaneIsOpen()).toBe(false);
        await convergeLivePane(driver, seed, projectId, pageId);
        // No headings, no preview affordance: the collapsed control needs
        // something to summarize before it takes up space.
        expect(await driver.pagesPanePreviewVisible()).toBe(false);
        expect(await driver.pagesPanePreviewBarCount()).toBe(0);
        await driver.pagesPaneEditorPasteHtml("<p>Lead para</p><h1>Alpha top</h1><h2>Beta mid</h2><h3>Gamma low</h3>");
        await expect.poll(() => driver.pagesPanePreviewBarCount(), { timeout: 30_000 }).toBe(3);
        expect(await driver.pagesPanePreviewVisible()).toBe(true);
      });

      await test.step("hover expands the heading card", async () => {
        expect(await driver.pagesPanePreviewCardVisible()).toBe(false);
        await driver.pagesPanePreviewHover();
        await expect.poll(() => driver.pagesPanePreviewCardVisible(), { timeout: 30_000 }).toBe(true);
        expect(await driver.pagesPanePreviewCardHeadings()).toEqual(["Alpha top", "Beta mid", "Gamma low"]);
      });

      await test.step("selecting the preview opens the pane on the outline", async () => {
        await driver.pagesPanePreviewOpen();
        expect(await driver.pagesPaneIsOpen()).toBe(true);
        expect(await driver.pagesPaneQueryParam("paneTab")).toBe("outline");
        expect(await driver.pagesPaneOutlineHeadings()).toEqual([
          { level: 1, text: "Alpha top" },
          { level: 2, text: "Beta mid" },
          { level: 3, text: "Gamma low" },
        ]);
        await driver.pagesPaneClose();
      });

      await test.step("adding a heading updates the preview without reload", async () => {
        // A lone pasted heading folds its text into the current block
        // instead of adding a node, so the addition is typed instead.
        await driver.pagesPaneEditorClickEnd();
        await driver.pagesPaneEditorPressEnter();
        await driver.pagesPaneEditorType("## Delta added");
        await driver.pagesPaneEditorPressEnter();
        await expect.poll(() => driver.pagesPanePreviewBarCount(), { timeout: 30_000 }).toBe(4);
        await driver.pagesPanePreviewHover();
        await expect.poll(() => driver.pagesPanePreviewCardHeadings(), { timeout: 30_000 }).toContain("Delta added");
        expect(await driver.pagesPaneQueryParam("paneTab")).toBe(null);
      });

      await test.step("typing inside a heading updates the preview text live", async () => {
        await driver.pagesPaneEditorClickText("Gamma low");
        await driver.pagesPaneEditorType(" live");
        await expect
          .poll(async () => (await driver.pagesPanePreviewCardHeadings()).some((name) => name.includes("live")), {
            timeout: 30_000,
          })
          .toBe(true);
      });

      await test.step("the preview reads the live editor only", async () => {
        expect((await serverPageRecord(seed.workspaceSlug, projectId, pageId, session)).id).toBe(pageId);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-047"], "outline tab lists three heading levels live with click-to-scroll and an empty prompt"),
  { tag: specTags(["PAGE-047"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "outline", session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("outline")} page`);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.pagesPaneOpenDetail(seed.workspaceSlug, projectId, pageId);

      await test.step("empty documents show the themed headings prompt", async () => {
        await convergeLivePane(driver, seed, projectId, pageId);
        await driver.pagesPaneOpenViaToolbar();
        expect(await driver.pagesPaneOutlineEmptyVisible()).toBe(true);
        expect(await driver.pagesPaneOutlineHeadings()).toEqual([]);
      });

      const filler = (from: number, count: number): string =>
        Array.from({ length: count }, (_, i) => `<p>filler ${from + i}</p>`).join("");
      await test.step("first- through third-level headings list live with levels", async () => {
        await driver.pagesPaneEditorPasteHtml(
          `<h1>Top anchor</h1>${filler(1, 30)}<h2>Mid anchor</h2>${filler(31, 30)}<h3>Low anchor</h3>`
        );
        await expect
          .poll(() => driver.pagesPaneOutlineHeadings(), { timeout: 30_000 })
          .toEqual([
            { level: 1, text: "Top anchor" },
            { level: 2, text: "Mid anchor" },
            { level: 3, text: "Low anchor" },
          ]);
        expect(await driver.pagesPaneOutlineEmptyVisible()).toBe(false);
      });

      await test.step("clicking a heading scrolls the canvas to it", async () => {
        const before = await driver.pagesPaneEditorScrollTop();
        await driver.pagesPaneOutlineClick("Low anchor");
        await expect.poll(() => driver.pagesPaneEditorScrollTop(), { timeout: 15_000 }).toBeGreaterThan(before + 100);
      });

      await test.step("headings appear as they are added, without reload", async () => {
        // A lone pasted heading folds its text into the current block
        // instead of adding a node, so the addition is typed instead.
        await driver.pagesPaneEditorClickEnd();
        await driver.pagesPaneEditorPressEnter();
        await driver.pagesPaneEditorType("# Live top");
        await driver.pagesPaneEditorPressEnter();
        await expect
          .poll(async () => (await driver.pagesPaneOutlineHeadings()).map((row) => row.text), { timeout: 30_000 })
          .toContain("Live top");
      });

      await test.step("typing inside a heading updates its outline text", async () => {
        await driver.pagesPaneEditorClickText("Mid anchor");
        await driver.pagesPaneEditorType(" live");
        await expect
          .poll(async () => (await driver.pagesPaneOutlineHeadings()).some((row) => row.text.includes("live")), {
            timeout: 30_000,
          })
          .toBe(true);
      });

      await test.step("the outline reads the live editor only", async () => {
        expect((await serverPageRecord(seed.workspaceSlug, projectId, pageId, session)).id).toBe(pageId);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-048"], "assets tab lists image rows with thumbnails and a hover download"),
  { tag: specTags(["PAGE-048"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "assets", session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("assets")} page`);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.pagesPaneOpenDetail(seed.workspaceSlug, projectId, pageId);
      await convergeLivePane(driver, seed, projectId, pageId);
      await driver.pagesPaneOpenViaToolbar();
      await driver.pagesPaneSelectTab("Assets");

      await test.step("no attachments show the empty illustration", async () => {
        expect(await driver.pagesPaneAssetsEmptyVisible()).toBe(true);
        expect(await driver.pagesPaneAssetsRows()).toEqual([]);
      });

      const src = "https://example.com/nc317-sunset.png";
      await test.step("an added image renders a thumbnail row with view and download links", async () => {
        await driver.pagesPaneEditorPasteHtml(`<p>See below</p><img src="${src}" alt="nc317 sunset">`);
        await expect.poll(() => driver.pagesPaneAssetsRows(), { timeout: 30_000 }).toHaveLength(1);
        const rows = await driver.pagesPaneAssetsRows();
        const row = rows[0];
        if (row === undefined) throw new Error("[parity] assets tab lost its row between reads.");
        expect(row.name.length).toBeGreaterThan(0);
        expect(row.thumbnail).toBe(true);
        expect(row.viewHref.length).toBeGreaterThan(0);
        expect(row.downloadHref).toContain(src);
        expect(await driver.pagesPaneAssetsEmptyVisible()).toBe(false);
      });

      await test.step("hover reveals the download action", async () => {
        const rows = await driver.pagesPaneAssetsRows();
        const row = rows[0];
        if (row === undefined) throw new Error("[parity] assets tab lost its row between reads.");
        expect(await driver.pagesPaneAssetsDownloadVisible(row.name)).toBe(true);
      });

      await test.step("the tab reads editor assets only", async () => {
        expect((await serverPageRecord(seed.workspaceSlug, projectId, pageId, session)).id).toBe(pageId);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-048"], "removing the image clears the assets tab; plain links never become rows"),
  { tag: specTags(["PAGE-048"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "assets remove", session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("assets remove")} page`);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.pagesPaneOpenDetail(seed.workspaceSlug, projectId, pageId);
      await convergeLivePane(driver, seed, projectId, pageId);
      await driver.pagesPaneOpenViaToolbar();
      await driver.pagesPaneSelectTab("Assets");

      await test.step("an added image renders one row", async () => {
        await driver.pagesPaneEditorPasteHtml(
          '<p>See below</p><img src="https://example.com/nc317-hill.png" alt="nc317 hill">'
        );
        await expect.poll(() => driver.pagesPaneAssetsRows(), { timeout: 30_000 }).toHaveLength(1);
      });

      await test.step("a file link does not leak in as an asset row", async () => {
        await driver.pagesPaneEditorPasteHtml('<p><a href="https://example.com/nc317-doc.pdf">nc317 doc</a></p>');
        await expect.poll(() => driver.pagesPaneEditorText(), { timeout: 30_000 }).toContain("nc317 doc");
        // The pane follows the editor a beat behind (observer plus frame),
        // so the settled row count is polled like its sibling steps instead
        // of racing the update with a single read (NEWFRONT-941).
        await expect.poll(() => driver.pagesPaneAssetsRows(), { timeout: 30_000 }).toHaveLength(1);
      });

      await test.step("removing the image from the document clears the tab", async () => {
        await driver.pagesPaneEditorDeleteImage();
        await expect.poll(() => driver.pagesPaneAssetsRows(), { timeout: 30_000 }).toEqual([]);
        expect(await driver.pagesPaneAssetsEmptyVisible()).toBe(true);
      });

      await test.step("the tab reads editor assets only", async () => {
        expect((await serverPageRecord(seed.workspaceSlug, projectId, pageId, session)).id).toBe(pageId);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-049"], "info tab statistics update live while typing, with seconds and minutes read times"),
  { tag: specTags(["PAGE-049"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "stats", session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("stats")} page`);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.pagesPaneOpenDetail(seed.workspaceSlug, projectId, pageId);
      await convergeLivePane(driver, seed, projectId, pageId);
      await driver.pagesPaneOpenViaToolbar();
      await driver.pagesPaneSelectTab("Info");

      const baseline = await driver.pagesPaneStats();
      await test.step("typing moves the word and character counts", async () => {
        await driver.pagesPaneEditorClickEnd();
        await driver.pagesPaneEditorType("Hello brave world");
        await expect
          .poll(() => driver.pagesPaneStats(), { timeout: 30_000 })
          .toMatchObject({ words: baseline.words + 3 });
        const after = await driver.pagesPaneStats();
        expect(after.characters).toBeGreaterThan(baseline.characters);
        expect(after.paragraphs).toBeGreaterThanOrEqual(baseline.paragraphs);
        expect(after.readTime).toMatch(/^\d+s$/);
      });

      await test.step("long documents estimate the read time in minutes", async () => {
        const lorem = Array.from({ length: 400 }, () => "lorem").join(" ");
        await driver.pagesPaneEditorPasteHtml(`<p>${lorem}</p>`);
        await expect.poll(async () => (await driver.pagesPaneStats()).readTime, { timeout: 30_000 }).toMatch(/^\d+m$/);
        const after = await driver.pagesPaneStats();
        expect(after.words).toBeGreaterThan(baseline.words + 390);
      });

      await test.step("statistics subscribe to the editor only", async () => {
        expect((await serverPageRecord(seed.workspaceSlug, projectId, pageId, session)).id).toBe(pageId);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-050"], "info tab attributes last editor and creator with profile links"),
  { tag: specTags(["PAGE-050"]) },
  async ({ driver, seed }) => {
    const member = requireMentionMember(seed);
    const session = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(member.email, member.password);
    const { projectId } = await scratchProject(seed, "actors", session);
    await serverEnsureProjectMember(seed.workspaceSlug, projectId, member.email, session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("actors")} page`);
    try {
      await test.step("a member edit moves the last-editor attribution server-side", async () => {
        await serverUpdatePageDescription(
          seed.workspaceSlug,
          projectId,
          pageId,
          memberSession,
          htmlParagraphs("Member words here"),
          tiptapParagraphs("Member words here")
        );
        const record = await serverPageRecord(seed.workspaceSlug, projectId, pageId, session);
        expect(record.updatedBy).toBe(member.id);
      });

      // Opens as the editing member: the viewer is the last editor, so the
      // attribution cannot drift no matter when the live store persists.
      await driver.rulesEnsureSignedIn(member.email, member.password, seed.workspaceSlug);
      await driver.pagesPaneOpenDetail(seed.workspaceSlug, projectId, pageId);
      await driver.pagesPaneOpenViaToolbar();
      await driver.pagesPaneSelectTab("Info");

      await test.step("editor and creator rows link to profiles with relative and calendar times", async () => {
        const record = await serverPageRecord(seed.workspaceSlug, projectId, pageId, session);
        expect(record.updatedBy).toBe(member.id);
        const actors = await driver.pagesPaneActors();
        expect(actors).toHaveLength(2);
        const edited = actors.find((row) => row.role === "edited");
        const created = actors.find((row) => row.role === "created");
        if (edited === undefined || created === undefined) throw new Error("[parity] actor rows missing a role.");
        expect(edited.name).toContain(member.displayName);
        expect(edited.profileHref).toContain(`/profile/${member.id}`);
        expect(edited.timeText).toContain("ago");
        expect(created.name.length).toBeGreaterThan(0);
        expect(created.profileHref).toContain(`/profile/${record.ownedBy}`);
        expect(created.timeText).toMatch(/\d{4}/);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-050"], "a missing editor falls back to a deactivated-user label without breaking the rows"),
  { tag: specTags(["PAGE-050"]) },
  async ({ driver, seed }) => {
    const member = requireMentionMember(seed);
    const session = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(member.email, member.password);
    const { projectId } = await scratchProject(seed, "actors fallback", session);
    await serverEnsureProjectMember(seed.workspaceSlug, projectId, member.email, session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("actors fallback")} page`);
    try {
      await test.step("a member edit moves the last-editor attribution server-side", async () => {
        await serverUpdatePageDescription(
          seed.workspaceSlug,
          projectId,
          pageId,
          memberSession,
          htmlParagraphs("Member words here"),
          tiptapParagraphs("Member words here")
        );
        const record = await serverPageRecord(seed.workspaceSlug, projectId, pageId, session);
        expect(record.updatedBy).toBe(member.id);
      });

      // The omission stages before the only open, and the view is the
      // editing member's own: reopening rewrites the attribution
      // server-side (NEWFRONT-340), so the fallback is read on the first
      // load with the member missing from the member lists.
      await driver.pagesPaneOmitWorkspaceMember(member.id);
      await driver.rulesEnsureSignedIn(member.email, member.password, seed.workspaceSlug);
      await driver.pagesPaneOpenDetailQuery(seed.workspaceSlug, projectId, pageId, "paneTab=info");

      await test.step("a missing user falls back without breaking the rows", async () => {
        const record = await serverPageRecord(seed.workspaceSlug, projectId, pageId, session);
        expect(record.updatedBy).toBe(member.id);
        const actors = await driver.pagesPaneActors();
        expect(actors).toHaveLength(2);
        const edited = actors.find((row) => row.role === "edited");
        const created = actors.find((row) => row.role === "created");
        if (edited === undefined || created === undefined) throw new Error("[parity] actor rows missing a role.");
        expect(edited.name).toContain("Deactivated user");
        expect(edited.profileHref).toContain(`/profile/${member.id}`);
        expect(edited.timeText).toContain("ago");
        expect(created.name.length).toBeGreaterThan(0);
        expect(created.profileHref).toContain(`/profile/${record.ownedBy}`);
        expect(created.timeText).toMatch(/\d{4}/);
        await driver.pagesPaneRestoreWorkspaceMembers();
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-051"], "version timeline lists snapshots with authors; selection deep-links and opens the overlay"),
  { tag: specTags(["PAGE-051"]) },
  async ({ driver, seed }) => {
    const member = requireMentionMember(seed);
    const session = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(member.email, member.password);
    const { projectId } = await scratchProject(seed, "timeline", session);
    await serverEnsureProjectMember(seed.workspaceSlug, projectId, member.email, session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("timeline")} page`);
    try {
      await test.step("two authors persist two snapshots server-side", async () => {
        await serverUpdatePageDescription(
          seed.workspaceSlug,
          projectId,
          pageId,
          session,
          htmlParagraphs("Alpha version one"),
          tiptapParagraphs("Alpha version one")
        );
        // Serialize on the worker: each snapshot reads current page state
        // at task time, so the second write waits for the first snapshot.
        expect(await waitVersions(seed, projectId, pageId, session, 1)).toHaveLength(1);
        await serverUpdatePageDescription(
          seed.workspaceSlug,
          projectId,
          pageId,
          memberSession,
          htmlParagraphs("Beta version two"),
          tiptapParagraphs("Beta version two")
        );
        expect(await waitVersions(seed, projectId, pageId, session, 2)).toHaveLength(2);
      });

      // Views as the member who wrote last: the live store coalesces into
      // the viewer's latest same-author snapshot, so viewing as the owner
      // would rewrite the owner's snapshot with the member's content.
      await driver.rulesEnsureSignedIn(member.email, member.password, seed.workspaceSlug);
      await driver.pagesPaneOpenDetail(seed.workspaceSlug, projectId, pageId);
      await driver.pagesPaneOpenViaToolbar();
      await driver.pagesPaneSelectTab("Info");

      await test.step("the timeline shows the current row plus dated authored entries", async () => {
        const serverRows = await serverPageVersions(seed.workspaceSlug, projectId, pageId, session);
        const entries = await driver.pagesPaneVersionEntries();
        expect(entries).toHaveLength(serverRows.length + 1);
        const current = entries.find((row) => row.id === null);
        if (current === undefined) throw new Error("[parity] current-version row missing.");
        expect(current.label).toBe("Current version");
        expect(current.active).toBe(true);
        const snapshots = entries.filter((row) => row.id !== null);
        expect(new Set(snapshots.map((row) => row.id as string))).toEqual(new Set(serverRows.map((row) => row.id)));
        const byMember = snapshots.find(
          (row) => serverRows.find((server) => server.id === row.id)?.ownedBy === member.id
        );
        if (byMember === undefined) throw new Error("[parity] member snapshot missing from the timeline.");
        expect(byMember.label.length).toBeGreaterThan(0);
        expect(byMember.author).toContain(member.displayName);
        expect(byMember.active).toBe(false);
      });

      const serverRows = await serverPageVersions(seed.workspaceSlug, projectId, pageId, session);
      const memberRow = serverRows.find((row) => row.ownedBy === member.id);
      if (memberRow === undefined) throw new Error("[parity] member snapshot missing server-side.");
      await test.step("selecting an entry deep-links the version key and opens the overlay", async () => {
        await driver.pagesPaneSelectVersion(memberRow.id);
        expect(await driver.pagesPaneQueryParam("version")).toBe(memberRow.id);
        expect(await driver.pagesPaneOverlayOpen()).toBe(true);
        const entries = await driver.pagesPaneVersionEntries();
        expect(entries.find((row) => row.id === memberRow.id)?.active).toBe(true);
        expect(entries.find((row) => row.id === null)?.active).toBe(false);
        await driver.pagesPaneSelectCurrentVersion();
        expect(await driver.pagesPaneQueryParam("version")).toBe(null);
        expect(await driver.pagesPaneOverlayOpen()).toBe(false);
      });

      const ownerRow = serverRows.find((row) => row.ownedBy !== member.id);
      if (ownerRow === undefined) throw new Error("[parity] owner snapshot missing server-side.");
      await test.step("a deep link opens the overlay on its snapshot", async () => {
        await driver.pagesPaneOpenDetailQuery(
          seed.workspaceSlug,
          projectId,
          pageId,
          `paneTab=info&version=${ownerRow.id}`
        );
        await expect.poll(() => driver.pagesPaneOverlayOpen(), { timeout: 30_000 }).toBe(true);
        await expect.poll(() => driver.pagesPaneOverlayText(), { timeout: 60_000 }).toContain("Alpha version one");
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-052"], "overlay shows the snapshot timestamp read-only with a skeleton and content fallbacks"),
  { tag: specTags(["PAGE-052"]) },
  async ({ driver, seed }) => {
    const member = requireMentionMember(seed);
    const session = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(member.email, member.password);
    const { projectId } = await scratchProject(seed, "overlay", session);
    await serverEnsureProjectMember(seed.workspaceSlug, projectId, member.email, session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("overlay")} page`);
    try {
      // Both payloads agree on purpose: the server canonicalizes the
      // structured payload from the markup asynchronously, so divergent
      // payloads cannot survive to the overlay either way.
      await test.step("markup-only and structured snapshots persist server-side", async () => {
        await serverUpdatePageDescription(
          seed.workspaceSlug,
          projectId,
          pageId,
          memberSession,
          htmlParagraphs("Markup fallback words")
        );
        // Serialize on the worker: each snapshot reads current page state
        // at task time, so the second write waits for the first snapshot.
        expect(await waitVersions(seed, projectId, pageId, session, 1)).toHaveLength(1);
        await serverUpdatePageDescription(
          seed.workspaceSlug,
          projectId,
          pageId,
          session,
          htmlParagraphs("Snapshot shared words"),
          tiptapParagraphs("Snapshot shared words")
        );
        expect(await waitVersions(seed, projectId, pageId, session, 2)).toHaveLength(2);
      });

      const serverRows = await serverPageVersions(seed.workspaceSlug, projectId, pageId, session);
      const structured = serverRows.find((row) => row.ownedBy !== member.id);
      const markupOnly = serverRows.find((row) => row.ownedBy === member.id);
      if (structured === undefined || markupOnly === undefined) {
        throw new Error("[parity] expected one snapshot per author.");
      }
      await waitVersionDetail(seed, projectId, pageId, structured.id, session, {
        html: "Snapshot shared words",
        json: "Snapshot shared words",
      });
      await waitVersionDetail(seed, projectId, pageId, markupOnly.id, session, { html: "Markup fallback words" });

      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.pagesPaneOpenDetail(seed.workspaceSlug, projectId, pageId);
      await driver.pagesPaneOpenViaToolbar();
      await driver.pagesPaneSelectTab("Info");
      const liveBefore = await driver.pagesPaneEditorText();

      await test.step("the snapshot renders with a timestamp and read-only marker", async () => {
        await driver.pagesPaneSelectVersion(structured.id);
        await expect.poll(() => driver.pagesPaneOverlayText(), { timeout: 60_000 }).toContain("Snapshot shared words");
        expect(await driver.pagesPaneOverlayText()).not.toContain("Markup fallback words");
        const header = await driver.pagesPaneOverlayHeader();
        expect(header.viewOnly).toBe(true);
        expect(header.title).toMatch(/\d{4}/);
        expect(await driver.pagesPaneEditorText()).toBe(liveBefore);
        await driver.pagesPaneSelectCurrentVersion();
      });

      await test.step("the skeleton shows while fetching, then markup renders as fallback", async () => {
        await driver.pagesPaneOverlayStallDetail();
        await driver.pagesPaneSelectVersion(markupOnly.id);
        await expect.poll(() => driver.pagesPaneOverlaySkeletonVisible(), { timeout: 30_000 }).toBe(true);
        expect((await driver.pagesPaneOverlayHeader()).title).toBe("Loading version details");
        await driver.pagesPaneOverlayReleaseDetail();
        await expect.poll(() => driver.pagesPaneOverlaySkeletonVisible(), { timeout: 60_000 }).toBe(false);
        await expect.poll(() => driver.pagesPaneOverlayText(), { timeout: 60_000 }).toContain("Markup fallback words");
        await driver.pagesPaneSelectCurrentVersion();
      });

      await test.step("the server detail carries both payloads", async () => {
        const detail = await serverPageVersionDetail(seed.workspaceSlug, projectId, pageId, structured.id, session);
        expect(detail.html).toContain("Snapshot shared words");
        expect(detail.jsonText ?? "").toContain("Snapshot shared words");
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-053"], "restore writes the snapshot into the live editor in place with a success notice"),
  { tag: specTags(["PAGE-053"]) },
  async ({ driver, seed }) => {
    const member = requireMentionMember(seed);
    const session = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(member.email, member.password);
    const { projectId } = await scratchProject(seed, "restore", session);
    await serverEnsureProjectMember(seed.workspaceSlug, projectId, member.email, session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("restore")} page`);
    try {
      await test.step("a snapshot persists server-side", async () => {
        await serverUpdatePageDescription(
          seed.workspaceSlug,
          projectId,
          pageId,
          memberSession,
          htmlParagraphs("Snapshot restore me"),
          tiptapParagraphs("Snapshot restore me")
        );
        expect(await waitVersions(seed, projectId, pageId, session, 1)).toHaveLength(1);
      });
      const snapshot = (await serverPageVersions(seed.workspaceSlug, projectId, pageId, session))[0];
      if (snapshot === undefined) throw new Error("[parity] snapshot missing server-side.");
      await waitVersionDetail(seed, projectId, pageId, snapshot.id, session, { html: "Snapshot restore me" });

      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.pagesPaneOpenDetail(seed.workspaceSlug, projectId, pageId);
      await test.step("live content diverges from the snapshot", async () => {
        await driver.pagesPaneEditorClickEnd();
        await driver.pagesPaneEditorType("Live content here");
        await expect.poll(() => driver.pagesPaneEditorText(), { timeout: 30_000 }).toContain("Live content here");
      });

      await driver.pagesPaneOpenViaToolbar();
      await driver.pagesPaneSelectTab("Info");
      const pathBefore = await driver.currentPath();
      await test.step("restore replaces live content without navigating away", async () => {
        await driver.pagesPaneSelectVersion(snapshot.id);
        // The control appears only once the snapshot read lands (spec
        // §6.9), so the assertion waits for it instead of racing the
        // fetch with a single read (NEWFRONT-941).
        await expect.poll(() => driver.pagesPaneRestoreVisible(), { timeout: 30_000 }).toBe(true);
        await driver.pagesPaneRestore();
        expect(await driver.pagesPaneOverlayOpen()).toBe(false);
        expect(await driver.pagesPaneQueryParam("version")).toBe(null);
        expect(await toastText(driver)).toContain("restored");
        await expect.poll(() => driver.pagesPaneEditorText(), { timeout: 30_000 }).toContain("Snapshot restore me");
        expect(await driver.pagesPaneEditorText()).not.toContain("Live content here");
        const pathAfter = await driver.currentPath();
        expect(new URL(pathAfter, "http://parity.invalid").pathname).toBe(
          new URL(pathBefore, "http://parity.invalid").pathname
        );
      });

      await test.step("the restored content persists server-side", async () => {
        await waitDescriptionContains(seed, projectId, pageId, session, "Snapshot restore me");
        const binary = await serverPageDescription(seed.workspaceSlug, projectId, pageId, session);
        expect(binary.byteLength).toBeGreaterThan(0);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-053"], "read-only viewers can view snapshots but never restore"),
  { tag: specTags(["PAGE-053"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "readonly", session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("readonly")} page`);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.pagesPaneOpenDetail(seed.workspaceSlug, projectId, pageId);
      await test.step("the owner types live content and snapshots it", async () => {
        await driver.pagesPaneEditorClickEnd();
        await driver.pagesPaneEditorType("Owner typed words");
        await waitDescriptionContains(seed, projectId, pageId, session, "Owner typed words");
        await serverUpdatePageDescription(
          seed.workspaceSlug,
          projectId,
          pageId,
          session,
          htmlParagraphs("Owner snapshot words"),
          tiptapParagraphs("Owner snapshot words")
        );
        expect(await waitVersions(seed, projectId, pageId, session, 1)).toHaveLength(1);
      });
      const snapshot = (await serverPageVersions(seed.workspaceSlug, projectId, pageId, session))[0];
      if (snapshot === undefined) throw new Error("[parity] snapshot missing server-side.");
      await waitVersionDetail(seed, projectId, pageId, snapshot.id, session, { html: "Owner snapshot words" });

      // The lock stages before the read-only view, so the fresh open below
      // reads it on first paint; locked pages stay readable but never
      // offer restore, whoever is viewing.
      await test.step("locking the page makes every viewer read-only", async () => {
        await serverPageLock(seed.workspaceSlug, projectId, pageId, session);
        expect((await serverPageRecord(seed.workspaceSlug, projectId, pageId, session)).isLocked).toBe(true);
      });

      await test.step("a read-only viewer sees snapshots without a restore control", async () => {
        await driver.pagesPaneOpenDetailReadOnly(seed.workspaceSlug, projectId, pageId);
        await expect.poll(() => driver.pagesPaneEditorText(), { timeout: 60_000 }).toContain("Owner typed words");
        await driver.pagesPaneOpenViaToolbar();
        await driver.pagesPaneSelectTab("Info");
        await driver.pagesPaneSelectVersion(snapshot.id);
        await expect.poll(() => driver.pagesPaneOverlayText(), { timeout: 60_000 }).toContain("Owner snapshot words");
        expect(await driver.pagesPaneRestoreVisible()).toBe(false);
        await driver.pagesPaneSelectCurrentVersion();
        expect(await driver.pagesPaneQueryParam("version")).toBe(null);
      });

      await test.step("the server still attributes the page to its owner alone", async () => {
        const record = await serverPageRecord(seed.workspaceSlug, projectId, pageId, session);
        expect(record.updatedBy).toBe(record.ownedBy);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-054"], "version load failure shows an error panel with retry; closing strips the version key"),
  { tag: specTags(["PAGE-054"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "version error", session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("version error")} page`);
    try {
      await test.step("a snapshot persists server-side", async () => {
        await serverUpdatePageDescription(
          seed.workspaceSlug,
          projectId,
          pageId,
          session,
          htmlParagraphs("Retry snapshot words"),
          tiptapParagraphs("Retry snapshot words")
        );
        expect(await waitVersions(seed, projectId, pageId, session, 1)).toHaveLength(1);
      });
      const snapshot = (await serverPageVersions(seed.workspaceSlug, projectId, pageId, session))[0];
      if (snapshot === undefined) throw new Error("[parity] snapshot missing server-side.");

      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.pagesPaneOpenDetail(seed.workspaceSlug, projectId, pageId);
      await driver.pagesPaneEditorClickEnd();
      await driver.pagesPaneEditorType("Live doc words");
      await expect.poll(() => driver.pagesPaneEditorText(), { timeout: 30_000 }).toContain("Live doc words");
      await driver.pagesPaneOpenViaToolbar();
      await driver.pagesPaneSelectTab("Info");

      await test.step("a failed fetch shows the error panel", async () => {
        await driver.pagesPaneFailVersionDetail();
        await driver.pagesPaneSelectVersion(snapshot.id);
        await expect.poll(() => driver.pagesPaneOverlayErrorVisible(), { timeout: 30_000 }).toBe(true);
        expect(await driver.pagesPaneOverlayErrorTexts()).toEqual({
          heading: "Something went wrong!",
          body: "The version could not be loaded, please try again.",
          retry: "Try again",
        });
        expect(await driver.pagesPaneVersionDetailRequests()).toBeGreaterThanOrEqual(1);
      });

      await test.step("retry refetches and renders the snapshot", async () => {
        const before = await driver.pagesPaneVersionDetailRequests();
        await driver.pagesPaneUnfailVersionDetail();
        await driver.pagesPaneOverlayRetry();
        await expect.poll(() => driver.pagesPaneOverlayErrorVisible(), { timeout: 60_000 }).toBe(false);
        await expect.poll(() => driver.pagesPaneOverlayText(), { timeout: 60_000 }).toContain("Retry snapshot words");
        expect(await driver.pagesPaneVersionDetailRequests()).toBeGreaterThan(before);
      });

      await test.step("closing the overlay strips the version key and returns to the live document", async () => {
        await driver.pagesPaneSelectCurrentVersion();
        expect(await driver.pagesPaneQueryParam("version")).toBe(null);
        expect(await driver.pagesPaneQueryParam("paneTab")).toBe("info");
        expect(await driver.pagesPaneOverlayOpen()).toBe(false);
        expect(await driver.pagesPaneEditorText()).toContain("Live doc words");
      });

      await test.step("the server stayed healthy throughout", async () => {
        const detail = await serverPageVersionDetail(seed.workspaceSlug, projectId, pageId, snapshot.id, session);
        expect(detail.html).toContain("Retry snapshot words");
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
