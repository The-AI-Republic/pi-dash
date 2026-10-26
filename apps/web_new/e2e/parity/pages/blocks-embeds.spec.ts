// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-316): the page body block surface — the
// slash-command palette plus drag-handle block menu, fenced code blocks
// with the inline code mark, image uploads with the assets tab, work-item
// embeds, person mentions, body emoji, inline links, and the whole-body
// markup copy. Rows: PAGE-037–PAGE-044.
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  requireMentionMember,
  serverCleanupProject,
  serverCreatePage,
  serverCreateProjectWithFlags,
  serverEnsureProjectMember,
  serverPageRecord,
  serverPageVersionDetail,
  serverPageVersions,
  serverUpdatePageDescription,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";

test.use({ permissions: ["clipboard-read", "clipboard-write"] });

/** Unique area tag so scratch projects and page names never collide. */
function tag(area: string): string {
  return `NF316 ${area} ${Date.now()}`;
}

/** Scratch project for one scenario (deleted with the project in `finally`). */
async function scratchProject(
  seed: ParitySeedFacts,
  area: string,
  session: string
): Promise<{ projectId: string; name: string }> {
  // Project creation 400s transiently under stack load; a fresh mint retries.
  let error: unknown = null;
  for (let attempt = 0; attempt < 3; attempt++) {
    try {
      const name = `${tag(area)} project`;
      const projectId = await serverCreateProjectWithFlags(
        seed.workspaceSlug,
        name,
        parityProjectIdentifier("N316"),
        {},
        session
      );
      return { projectId, name };
    } catch (caught) {
      error = caught;
      await new Promise((resolve) => setTimeout(resolve, 2_000));
    }
  }
  throw error;
}

/** Best-effort cleanup that cannot hang the scenario past `timeoutMs`. */
async function cleanupProject(
  seed: ParitySeedFacts,
  projectId: string,
  session: string,
  timeoutMs = 30_000
): Promise<void> {
  await Promise.race([
    serverCleanupProject(seed.workspaceSlug, projectId, session).catch(() => {}),
    new Promise((resolve) => setTimeout(resolve, timeoutMs)),
  ]);
}

/** Poll the live record until its HTML contains `text` (autosave). */
async function waitDescriptionContains(
  seed: ParitySeedFacts,
  projectId: string,
  pageId: string,
  session: string,
  text: string,
  timeoutMs = 180_000
): Promise<string> {
  const started = Date.now();
  for (;;) {
    const record = await serverPageRecord(seed.workspaceSlug, projectId, pageId, session);
    if (record.descriptionHtml.includes(text)) return record.descriptionHtml;
    if (Date.now() - started > timeoutMs) {
      throw new Error(`[parity] description never contained ${JSON.stringify(text)}.`);
    }
    await new Promise((resolve) => setTimeout(resolve, 3_000));
  }
}

/** Poll the live record until its HTML stops changing (in-flight saves landed). */
async function waitDescriptionSettled(
  seed: ParitySeedFacts,
  projectId: string,
  pageId: string,
  session: string,
  quietMs = 20_000,
  timeoutMs = 180_000
): Promise<string> {
  const started = Date.now();
  let last = "";
  let stableSince = Date.now();
  for (;;) {
    const record = await serverPageRecord(seed.workspaceSlug, projectId, pageId, session);
    const now = Date.now();
    if (record.descriptionHtml !== last) {
      last = record.descriptionHtml;
      stableSince = now;
    }
    if (now - stableSince >= quietMs) return last;
    if (now - started > timeoutMs) {
      throw new Error("[parity] description never settled.");
    }
    await new Promise((resolve) => setTimeout(resolve, 2_000));
  }
}

/** Poll the live record until its HTML no longer contains `text` (autosave). */
async function waitDescriptionAbsent(
  seed: ParitySeedFacts,
  projectId: string,
  pageId: string,
  session: string,
  text: string,
  timeoutMs = 180_000
): Promise<string> {
  const started = Date.now();
  for (;;) {
    const record = await serverPageRecord(seed.workspaceSlug, projectId, pageId, session);
    if (!record.descriptionHtml.includes(text)) return record.descriptionHtml;
    if (Date.now() - started > timeoutMs) {
      throw new Error(`[parity] description still contained ${JSON.stringify(text)}.`);
    }
    await new Promise((resolve) => setTimeout(resolve, 3_000));
  }
}

/** Poll the version list until `count` snapshots persist (async worker). */
async function waitVersions(
  seed: ParitySeedFacts,
  projectId: string,
  pageId: string,
  session: string,
  count: number,
  timeoutMs = 180_000
): Promise<void> {
  const started = Date.now();
  for (;;) {
    const rows = await serverPageVersions(seed.workspaceSlug, projectId, pageId, session);
    if (rows.length >= count) return;
    if (Date.now() - started > timeoutMs) {
      throw new Error(`[parity] versions stuck below ${count}.`);
    }
    await new Promise((resolve) => setTimeout(resolve, 3_000));
  }
}

/** Poll one snapshot's JSON payload until it contains every `want` string. */
async function waitVersionJsonContains(
  seed: ParitySeedFacts,
  projectId: string,
  pageId: string,
  versionId: string,
  session: string,
  want: string[],
  timeoutMs = 180_000
): Promise<void> {
  const started = Date.now();
  for (;;) {
    const detail = await serverPageVersionDetail(seed.workspaceSlug, projectId, pageId, versionId, session);
    if (want.every((text) => (detail.jsonText ?? "").includes(text))) return;
    if (Date.now() - started > timeoutMs) {
      throw new Error("[parity] version snapshot JSON never carried the fixture.");
    }
    await new Promise((resolve) => setTimeout(resolve, 3_000));
  }
}

/** Poll the live editor until its text contains every marker (slow loads). */
async function waitEditorContains(driver: ParityDriver, markers: string[], timeoutMs = 90_000): Promise<void> {
  await expect
    .poll(
      async () => {
        const text = await driver.pagesBlocksEditorText();
        return markers.every((marker) => text.includes(marker));
      },
      { timeout: timeoutMs }
    )
    .toBe(true);
}

/**
 * Read the info-tab stats, recovering once from the stale editor-ref race
 * (NEWFRONT-380, same family as NEWFRONT-373): the tab can subscribe at load
 * to a stale store ref and report zeros for the whole page lifetime. One
 * lock cycle re-syncs the store, so the re-read skips the navigate to keep
 * that recovery instead of racing a fresh reload. Throws loudly otherwise.
 */
async function readInfoStatsHealthy(
  driver: ParityDriver,
  expectedCharacters: number
): Promise<{ words: number | null; characters: number | null }> {
  try {
    return await driver.pagesBlocksInfoStats(expectedCharacters);
  } catch {
    await driver.pagesBlocksHeaderLockCycle();
    try {
      return await driver.pagesBlocksInfoStats(expectedCharacters, true);
    } catch {
      throw new Error(
        `[parity] info stats never reached ${expectedCharacters} characters after lock-cycle recovery (NEWFRONT-380).`
      );
    }
  }
}

/** One-pixel PNG bytes for the upload fixtures. */
const PIXEL_PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==",
  "base64"
);

test(
  specTitle(
    ["PAGE-037"],
    "slash palette exposes the block catalog and every structural entry inserts, saves, and reloads"
  ),
  { tag: specTags(["PAGE-037"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "catalog", session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("catalog")} page`);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.pagesBlocksOpenDetail(seed.workspaceSlug, projectId, pageId);

      await test.step("the palette lists the full catalog", async () => {
        expect(await driver.pagesBlocksSlashItems()).toEqual([
          "Text",
          "Heading 1",
          "Heading 2",
          "Heading 3",
          "Heading 4",
          "Heading 5",
          "Heading 6",
          "Numbered list",
          "Bulleted list",
          "To-do list",
          "Table",
          "Quote",
          "Code",
          "Image",
          "Callout",
          "Divider",
          "Emoji",
          "Default",
          "Gray",
          "Peach",
          "Pink",
          "Orange",
          "Green",
          "Light blue",
          "Dark blue",
          "Purple",
          "Default background",
          "Gray",
          "Peach",
          "Pink",
          "Orange",
          "Green",
          "Light blue",
          "Dark blue",
          "Purple",
        ]);
      });

      const markers: string[] = [];
      const insertTextEntry = async (entry: string, marker: string): Promise<void> => {
        await driver.pagesBlocksSlashChoose(entry);
        await driver.pagesBlocksEditorType(marker);
        markers.push(marker);
        expect(await driver.pagesBlocksEditorText()).toContain(marker);
      };

      await test.step("every structural entry inserts", async () => {
        // Caret stays keyboard-driven throughout: clicking re-targets inside
        // lists and tables. Code exits via End+ArrowDown; the table goes
        // second-to-last so no entry must escape its cells.
        await driver.pagesBlocksEditorClickEnd();
        await insertTextEntry("Text", "m037-text");
        for (let level = 1; level <= 6; level++) {
          const marker = `m037-h${level}`;
          await driver.pagesBlocksSlashChoose(`Heading ${level}`);
          await driver.pagesBlocksEditorType(marker);
          markers.push(marker);
          const html = await driver.pagesBlocksEditorHtml();
          expect(html).toContain(`<h${level}`);
          expect(html).toContain(marker);
        }
        await insertTextEntry("Numbered list", "m037-numbered");
        expect(await driver.pagesBlocksEditorHtml()).toContain("<ol");
        await insertTextEntry("Bulleted list", "m037-bulleted");
        expect(await driver.pagesBlocksEditorHtml()).toContain("<ul");
        await insertTextEntry("To-do list", "m037-todo");
        expect(await driver.pagesBlocksEditorHtml()).toContain("taskList");
        await insertTextEntry("Quote", "m037-quote");
        expect(await driver.pagesBlocksEditorHtml()).toContain("<blockquote");
        await insertTextEntry("Callout", "m037-callout");
        expect((await driver.pagesBlocksEditorHtml()).toLowerCase()).toContain("callout");
        await driver.pagesBlocksUploadImage({ name: "catalog.png", mime: "image/png", bytes: PIXEL_PNG });
        const images = await driver.pagesBlocksInlineImages();
        expect(images.length).toBeGreaterThan(0);
        expect(images[0]?.src ?? "").toContain("/api/assets/v2");
        await driver.pagesBlocksEmojiOpen();
        expect((await driver.pagesBlocksEmojiItems()).length).toBeGreaterThan(0);
        await driver.pagesBlocksEmojiChoose(0);
        expect(await driver.pagesBlocksEditorHtml()).toContain('data-type="emoji"');
        await driver.pagesBlocksEditorPressKey("Enter");
        await driver.pagesBlocksEditorPressKey("Enter");
        await insertTextEntry("Code", "m037-code");
        expect(await driver.pagesBlocksEditorHtml()).toContain("<pre");
        await driver.pagesBlocksEditorPressKey("End");
        await driver.pagesBlocksEditorPressKey("ArrowDown");
        await insertTextEntry("Table", "m037-table");
        expect(await driver.pagesBlocksEditorHtml()).toContain("<table");
        await driver.pagesBlocksSlashChoose("Divider");
        expect(await driver.pagesBlocksEditorHtml()).toContain("horizontalRule");
      });

      await test.step("every entry saves and reloads", async () => {
        for (const marker of markers) {
          await waitDescriptionContains(seed, projectId, pageId, session, marker);
        }
        await driver.pagesBlocksReload();
        await waitEditorContains(driver, markers);
        const html = await driver.pagesBlocksEditorHtml();
        for (const marker of markers) expect(html).toContain(marker);
        expect(html).toContain("<h1");
        expect(html).toContain("<ol");
        expect(html).toContain("<ul");
        expect(html).toContain("taskList");
        expect(html).toContain("<table");
        expect(html).toContain("<blockquote");
        expect(html).toContain("<pre");
        expect(html.toLowerCase()).toContain("callout");
        expect(html).toContain("horizontalRule");
        expect(html).toContain("/api/assets/v2");
        expect(html).toContain('data-type="emoji"');
        const record = await serverPageRecord(seed.workspaceSlug, projectId, pageId, session);
        for (const marker of markers) expect(record.descriptionHtml).toContain(marker);
      });
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-037"], "letter and wash colors apply through the palette and persist"),
  { tag: specTags(["PAGE-037"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "colors", session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("colors")} page`);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.pagesBlocksOpenDetail(seed.workspaceSlug, projectId, pageId);

      const letter = ["Default", "Gray", "Peach", "Pink", "Orange", "Green", "Light blue", "Dark blue", "Purple"];
      const wash = [
        "Default background",
        "Gray",
        "Peach",
        "Pink",
        "Orange",
        "Green",
        "Light blue",
        "Dark blue",
        "Purple",
      ];
      const markers: string[] = [];

      await test.step("every letter color applies", async () => {
        await driver.pagesBlocksEditorClickEnd();
        for (const [index, name] of letter.entries()) {
          const marker = `m037l${index}`;
          await driver.pagesBlocksSlashChoose(name, 0);
          await driver.pagesBlocksEditorType(marker);
          markers.push(marker);
          expect(await driver.pagesBlocksEditorText()).toContain(marker);
        }
      });

      await test.step("every wash color applies", async () => {
        for (const [index, name] of wash.entries()) {
          const marker = `m037w${index}`;
          await driver.pagesBlocksSlashChoose(name, name === "Default background" ? 0 : 1);
          await driver.pagesBlocksEditorType(marker);
          markers.push(marker);
          expect(await driver.pagesBlocksEditorText()).toContain(marker);
        }
      });

      await test.step("colors save and reload", async () => {
        for (const marker of markers) {
          await waitDescriptionContains(seed, projectId, pageId, session, marker);
        }
        await driver.pagesBlocksReload();
        await waitEditorContains(driver, markers);
        const html = await driver.pagesBlocksEditorHtml();
        for (const marker of markers) expect(html).toContain(marker);
        // The two Default entries clear their mark; every other entry wraps its marker in a styled span.
        const styled = html.split("<span").length - 1;
        expect(styled).toBeGreaterThanOrEqual(16);
        const record = await serverPageRecord(seed.workspaceSlug, projectId, pageId, session);
        for (const marker of markers) expect(record.descriptionHtml).toContain(marker);
      });
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-037"], "drag handle exposes block actions and duplicate plus delete save and reload"),
  { tag: specTags(["PAGE-037"]) },
  async ({ driver, seed }, testInfo) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "drag", session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("drag")} page`);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await serverUpdatePageDescription(
        seed.workspaceSlug,
        projectId,
        pageId,
        session,
        "<p>drag first</p><p>drag last</p>",
        null
      );
      await driver.pagesBlocksOpenDetail(seed.workspaceSlug, projectId, pageId);
      await waitEditorContains(driver, ["drag first", "drag last"]);

      await test.step("the handle shows on hover with delete and duplicate actions", async () => {
        expect(await driver.pagesBlocksDragHandleVisible()).toBe(true);
        await driver.pagesBlocksDragOpenMenu();
        const items = await driver.pagesBlocksDragMenuItems();
        // The old menu carries duplicate and delete only. The area spec
        // (§6.1, PAGE-037) says the handle also exposes move-up and
        // move-down, and web_new implements that intended set; the two
        // legs pin their own catalogue rather than dropping either.
        if (testInfo.project.name === "new") {
          expect(items).toEqual(["Delete", "Duplicate", "Move up", "Move down"]);
        } else {
          expect(items).toEqual(["Delete", "Duplicate"]);
        }
        await driver.pagesBlocksEditorPressKey("Escape");
      });

      const countOccurrences = (text: string, needle: string): number => text.split(needle).length - 1;

      await test.step("duplicate clones the block and delete removes it", async () => {
        await driver.pagesBlocksDragOpenMenu();
        await driver.pagesBlocksDragMenuChoose("Duplicate");
        expect(countOccurrences(await driver.pagesBlocksEditorText(), "drag last")).toBe(2);
        await driver.pagesBlocksDragOpenMenu();
        await driver.pagesBlocksDragMenuChoose("Delete");
        expect(countOccurrences(await driver.pagesBlocksEditorText(), "drag last")).toBe(1);
        // Save marker: the poll must cover the delete, not an earlier save.
        await driver.pagesBlocksEditorClickEnd();
        await driver.pagesBlocksEditorType(" drag done");
      });

      await test.step("the surviving block saves and reloads", async () => {
        await waitDescriptionContains(seed, projectId, pageId, session, "drag done");
        await driver.pagesBlocksReload();
        const text = await driver.pagesBlocksEditorText();
        expect(countOccurrences(text, "drag last")).toBe(1);
        expect(text).toContain("drag first");
      });
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-037"], "markdown paste renders blocks and the bubble underline plus alignment apply"),
  { tag: specTags(["PAGE-037"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    // Project names reject hyphens (PROJECT_NAME_CANNOT_CONTAIN_SPECIAL_CHARACTERS).
    const { projectId } = await scratchProject(seed, "pastemarks", session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("pastemarks")} page`);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.pagesBlocksOpenDetail(seed.workspaceSlug, projectId, pageId);
      await driver.pagesBlocksEditorClickEnd();

      await test.step("pasted markdown becomes blocks", async () => {
        await driver.pagesBlocksEditorPasteText(
          "# Pasted Heading\n\n- item one\n- item two\n\n```js\nconst x = 1;\n```\n"
        );
        const html = await driver.pagesBlocksEditorHtml();
        expect(html).toContain("<h1");
        expect(html).toContain("Pasted Heading");
        expect(html).toContain("<ul");
        expect(html).toContain("<pre");
      });

      await test.step("bubble underline and center alignment apply", async () => {
        await driver.pagesBlocksParagraphSelectText("item one");
        await driver.pagesBlocksBubbleFormat(2);
        expect(await driver.pagesBlocksEditorHtml()).toContain("<u");
        await driver.pagesBlocksEditorPressKey("Escape");
        await driver.pagesBlocksParagraphSelectText("item two");
        await driver.pagesBlocksBubbleFormat(5);
        const html = await driver.pagesBlocksEditorHtml();
        expect(html.toLowerCase()).toContain("text-align");
        expect(html.toLowerCase()).toContain("center");
      });

      await test.step("marks save and reload", async () => {
        await waitDescriptionContains(seed, projectId, pageId, session, "Pasted Heading");
        await waitDescriptionContains(seed, projectId, pageId, session, "<u");
        await waitDescriptionContains(seed, projectId, pageId, session, "text-align");
        await driver.pagesBlocksReload();
        const html = await driver.pagesBlocksEditorHtml();
        expect(html).toContain("<h1");
        expect(html).toContain("<u");
        expect(html.toLowerCase()).toContain("center");
      });
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-037"], "info tab counts characters live as the body grows"),
  { tag: specTags(["PAGE-037"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "charcount", session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("charcount")} page`);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await serverUpdatePageDescription(seed.workspaceSlug, projectId, pageId, session, "<p>abcde</p>", null);
      await driver.pagesBlocksOpenDetail(seed.workspaceSlug, projectId, pageId);
      await waitEditorContains(driver, ["abcde"]);

      const before = await readInfoStatsHealthy(driver, 5);
      expect(before.characters).toBe(5);

      await driver.pagesBlocksOpenDetail(seed.workspaceSlug, projectId, pageId);
      await driver.pagesBlocksEditorClickEnd();
      await driver.pagesBlocksEditorType("fghij");
      await waitDescriptionContains(seed, projectId, pageId, session, "fghij");

      const after = await readInfoStatsHealthy(driver, 10);
      expect(after.characters).toBe(10);
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-038"], "fenced code blocks insert through the block menu and persist with the inline code mark"),
  { tag: specTags(["PAGE-038"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "code", session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("code")} page`);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await serverUpdatePageDescription(
        seed.workspaceSlug,
        projectId,
        pageId,
        session,
        "<p>run printf daily</p>",
        null
      );
      await driver.pagesBlocksOpenDetail(seed.workspaceSlug, projectId, pageId);
      await waitEditorContains(driver, ["run printf daily"]);
      await driver.pagesBlocksEditorClickEnd();

      await test.step("a fenced code block inserts", async () => {
        await driver.pagesBlocksSlashChoose("Code");
        await driver.pagesBlocksEditorType("const blocks = 038;");
        const html = await driver.pagesBlocksEditorHtml();
        expect(html).toContain("<pre");
        expect(html).toContain("const blocks = 038;");
      });

      await test.step("the inline code mark toggles on selected text", async () => {
        await driver.pagesBlocksParagraphSelectText("run printf daily");
        await driver.pagesBlocksEditorPressKey("ControlOrMeta+e");
        await driver.pagesBlocksEditorPressKey("Escape");
        const html = await driver.pagesBlocksEditorHtml();
        expect(html).toContain("<code");
        expect(html).toContain("printf");
        // Save marker: the poll must cover the mark, not the earlier block save.
        await driver.pagesBlocksEditorClickEnd();
        await driver.pagesBlocksEditorType(" m038-saved");
      });

      await test.step("code blocks and marks persist in the saved payload", async () => {
        const saved = await waitDescriptionContains(seed, projectId, pageId, session, "m038-saved");
        expect(saved).toContain("<pre");
        expect(saved).toContain("<code");
        expect(saved).toContain("printf");
        await driver.pagesBlocksReload();
        const html = await driver.pagesBlocksEditorHtml();
        expect(html).toContain("<pre");
        expect(html).toContain("const blocks = 038;");
        expect(html).toContain("<code");
      });
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-039"], "image upload renders inline and in the assets tab until removed from the document"),
  { tag: specTags(["PAGE-039"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "upload", session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("upload")} page`);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.pagesBlocksOpenDetail(seed.workspaceSlug, projectId, pageId);

      await test.step("upload attaches a fresh asset and renders inline", async () => {
        await driver.pagesBlocksUploadImage({ name: "inline.png", mime: "image/png", bytes: PIXEL_PNG });
        const images = await driver.pagesBlocksInlineImages();
        expect(images.length).toBe(1);
        expect(images[0]?.src ?? "").toContain("/api/assets/v2");
      });

      await test.step("the assets tab lists the upload", async () => {
        await driver.pagesBlocksAssetsTabOpen();
        await expect.poll(() => driver.pagesBlocksAssetsTabRows(), { timeout: 60_000 }).not.toEqual([]);
        const rows = await driver.pagesBlocksAssetsTabRows();
        expect(rows.some((row) => row.name.includes("image"))).toBe(true);
        expect(await driver.pagesBlocksAssetsTabEmptyVisible()).toBe(false);
      });

      await test.step("removing the image from the document clears the rows", async () => {
        await driver.pagesBlocksOpenDetail(seed.workspaceSlug, projectId, pageId);
        // The image block's drag handle sits under its hover overlays, so the
        // keyboard clears the image-only document instead (block-menu Delete
        // is covered on text blocks by the PAGE-037 drag scenario).
        await driver.pagesBlocksEditorClickEnd();
        await driver.pagesBlocksEditorSelectAll();
        await driver.pagesBlocksEditorPressKey("Backspace");
        expect(await driver.pagesBlocksInlineImages()).toEqual([]);
        await waitDescriptionAbsent(seed, projectId, pageId, session, "image-component");
        await driver.pagesBlocksAssetsTabOpen();
        await expect.poll(() => driver.pagesBlocksAssetsTabEmptyVisible(), { timeout: 60_000 }).toBe(true);
        expect(await driver.pagesBlocksAssetsTabRows()).toEqual([]);
      });
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-039"], "bug: NEWFRONT-351 pasting an image onto another page errors instead of cloning the asset"),
  { tag: specTags(["PAGE-039"]) },
  async ({ driver, seed }, testInfo) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "duplicate", session);
    const pageA = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("duplicate-a")} page`);
    const pageB = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("duplicate-b")} page`);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.pagesBlocksOpenDetail(seed.workspaceSlug, projectId, pageA);
      await driver.pagesBlocksUploadImage({ name: "source.png", mime: "image/png", bytes: PIXEL_PNG });
      const before = await driver.pagesBlocksInlineImages();
      expect(before.length).toBe(1);
      // The source save must land first, or the clone has nothing to copy.
      await waitDescriptionContains(seed, projectId, pageA, session, "image-component");

      await driver.pagesBlocksEditorSelectAll();
      await driver.pagesBlocksEditorCopy();
      await driver.pagesBlocksOpenDetail(seed.workspaceSlug, projectId, pageB);
      await driver.pagesBlocksEditorClickEnd();
      await driver.pagesBlocksEditorPaste();

      // Intended: a second inline image with a fresh asset id. The oracle
      // pins the old bug (the duplicate-assets call 500s, NEWFRONT-351, and
      // the node shows its error state); web_new implements the intended
      // clone, which Parity allows for a `bug:` scenario ("Old bugs"). Each
      // leg pins its own outcome here and nothing else in the scenario moves.
      if (testInfo.project.name === "new") {
        await expect.poll(() => driver.pagesBlocksInlineImages(), { timeout: 120_000 }).toHaveLength(1);
        const after = await driver.pagesBlocksInlineImages();
        expect(after).toHaveLength(1);
        expect(before[0]?.src).toBeTruthy();
        expect(after[0]?.src).toBeTruthy();
        expect(after[0]?.src).not.toBe(before[0]?.src);
        expect(await driver.pagesBlocksEditorText()).not.toContain("Error loading image");
      } else {
        await expect.poll(() => driver.pagesBlocksEditorText(), { timeout: 120_000 }).toContain("Error loading image");
        expect(await driver.pagesBlocksInlineImages()).toEqual([]);
      }
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(
    ["PAGE-040"],
    "bug: NEWFRONT-352 embed identifiers persist in snapshots while the community renders no upgrade card"
  ),
  { tag: specTags(["PAGE-040"]) },
  async ({ driver, seed }, testInfo) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "embed", session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("embed")} page`);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      const identifiers = {
        id: "emb-oracle-1",
        entity_identifier: "N316-ISS-1",
        project_identifier: "N316-PROJ",
        workspace_identifier: seed.workspaceSlug,
        entity_name: "Embedded issue",
      };

      await test.step("identifiers survive the save into the version snapshot", async () => {
        const json = JSON.stringify({
          type: "doc",
          content: [
            { type: "paragraph", content: [{ type: "text", text: "before embed" }] },
            { type: "issue-embed-component", attrs: identifiers },
            { type: "paragraph", content: [{ type: "text", text: "after embed" }] },
          ],
        });
        await serverUpdatePageDescription(seed.workspaceSlug, projectId, pageId, session, "<p>before embed</p>", json);
        await waitVersions(seed, projectId, pageId, session, 1);
        const rows = await serverPageVersions(seed.workspaceSlug, projectId, pageId, session);
        const latest = rows[rows.length - 1];
        if (latest === undefined) throw new Error("[parity] version list came back empty.");
        await waitVersionJsonContains(seed, projectId, pageId, latest.id, session, [
          "issue-embed-component",
          identifiers.id,
          identifiers.entity_identifier,
          identifiers.project_identifier,
          identifiers.workspace_identifier,
          identifiers.entity_name,
        ]);
      });

      await test.step("the community renders no embed slot or upgrade prompt", async () => {
        await driver.pagesBlocksOpenDetail(seed.workspaceSlug, projectId, pageId);
        await waitEditorContains(driver, ["before embed"]);
        // Intended: an upgrade prompt with an outbound link (NEWFRONT-352).
        // The oracle pins the old bug (nothing renders); web_new implements
        // the intended card, which Parity allows for a `bug:` scenario
        // ("Old bugs"). Snapshot persistence above stays shared.
        if (testInfo.project.name === "new") {
          expect(await driver.pagesBlocksEmbedUpgradeVisible()).toBe(true);
          expect(await driver.pagesBlocksEmbedUpgradeHref()).toBe("https://airepublic.com/pricing");
        } else {
          expect(await driver.pagesBlocksEmbedUpgradeVisible()).toBe(false);
          expect(await driver.pagesBlocksEmbedUpgradeHref()).toBe(null);
        }
        const text = await driver.pagesBlocksEditorText();
        expect(text).toContain("before embed");
      });
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-041"], "person mentions suggest, insert chips resolving display names, and survive save and reload"),
  { tag: specTags(["PAGE-041"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const member = requireMentionMember(seed);
    const { projectId } = await scratchProject(seed, "mention", session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("mention")} page`);
    // Suggestions are project-scoped: the second member must belong to the scratch project.
    await serverEnsureProjectMember(seed.workspaceSlug, projectId, member.email, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.pagesBlocksOpenDetail(seed.workspaceSlug, projectId, pageId);

      await test.step("suggestions list workspace members with display names", async () => {
        const rows = await driver.pagesBlocksMentionSuggest("");
        // "Parity Oracle" is the seed owner's display name (seed-defined).
        expect(rows.some((row) => row.includes("Parity Oracle"))).toBe(true);
        expect(rows.some((row) => row.includes(member.displayName))).toBe(true);
        expect(await driver.pagesBlocksMentionSections()).toContain("Users");
      });

      await test.step("choosing a suggestion inserts a chip with a profile link", async () => {
        await driver.pagesBlocksMentionChoose(member.displayName);
        expect(await driver.pagesBlocksMentionChips()).toEqual([`@${member.displayName}`]);
        const hrefs = await driver.pagesBlocksMentionChipHrefs();
        expect(hrefs.length).toBe(1);
        expect(hrefs[0] ?? "").toContain(`/profile/${member.id}`);
      });

      await test.step("the chip survives save and reload with its resolved name", async () => {
        const saved = await waitDescriptionContains(seed, projectId, pageId, session, "mention-component");
        expect(saved).toContain(member.id);
        await driver.pagesBlocksReload();
        expect(await driver.pagesBlocksMentionChips()).toEqual([`@${member.displayName}`]);
      });
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-042"], "body emoji inserts through the picker and persists"),
  { tag: specTags(["PAGE-042"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "emoji", session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("emoji")} page`);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.pagesBlocksOpenDetail(seed.workspaceSlug, projectId, pageId);

      let picked = "";
      await test.step("the picker inserts the chosen emoji inline", async () => {
        await driver.pagesBlocksEmojiOpen();
        const items = await driver.pagesBlocksEmojiItems();
        expect(items.length).toBeGreaterThan(0);
        picked = Array.from(items[0] ?? "")[0] ?? "";
        expect(picked).not.toBe("");
        await driver.pagesBlocksEmojiChoose(0);
        expect(await driver.pagesBlocksEditorHtml()).toContain('data-type="emoji"');
        expect(await driver.pagesBlocksEditorText()).toContain(picked);
      });

      await test.step("the emoji survives save and reload", async () => {
        await waitDescriptionContains(seed, projectId, pageId, session, "emoji");
        await driver.pagesBlocksReload();
        expect(await driver.pagesBlocksEditorHtml()).toContain('data-type="emoji"');
        expect(await driver.pagesBlocksEditorText()).toContain(picked);
      });
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-043"], "inline links edit in place and pasted addresses linkify without a rich preview"),
  { tag: specTags(["PAGE-043"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "links", session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("links")} page`);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await serverUpdatePageDescription(
        seed.workspaceSlug,
        projectId,
        pageId,
        session,
        '<p>see <a href="https://example.com/docs">the docs</a> here</p><p>pasted link target</p>',
        null
      );
      await driver.pagesBlocksOpenDetail(seed.workspaceSlug, projectId, pageId);
      await waitEditorContains(driver, ["the docs", "pasted link target"]);

      await test.step("the bubble edits the link address in place", async () => {
        await driver.pagesBlocksLinkSelect();
        expect(await driver.pagesBlocksLinkBubbleVisible()).toBe(true);
        expect(await driver.pagesBlocksLinkPrefill()).toBe("https://example.com/docs");
        await driver.pagesBlocksLinkEdit("https://example.com/edited");
        expect(await driver.pagesBlocksLinkHref()).toBe("https://example.com/edited");
      });

      await test.step("pasting an address over a selection links it", async () => {
        await driver.pagesBlocksParagraphSelectText("pasted link target");
        await driver.pagesBlocksEditorPasteText("https://example.com/pasted");
        const html = await driver.pagesBlocksEditorHtml();
        expect(html).toContain('href="https://example.com/pasted"');
        // Reload onto a fresh selection state: positioning the caret by click
        // while the pasted link holds a stale selection lets the next Enter
        // delete the link text, so the bare-address step starts clean.
        await waitDescriptionContains(seed, projectId, pageId, session, "https://example.com/pasted");
        await driver.pagesBlocksReload();
        await waitEditorContains(driver, ["pasted link target"]);
        expect(await driver.pagesBlocksEditorHtml()).toContain('href="https://example.com/pasted"');
      });

      await test.step("a bare pasted address autolinks without a preview card", async () => {
        await driver.pagesBlocksEditorClickEnd();
        await driver.pagesBlocksEditorPressEnter();
        await driver.pagesBlocksEditorPasteText("https://example.com/bare");
        const html = await driver.pagesBlocksEditorHtml();
        expect(html).toContain('href="https://example.com/bare"');
        expect(html).toContain(">https://example.com/bare</a>");
      });

      await test.step("edits save and reload", async () => {
        const hrefs = ["https://example.com/edited", "https://example.com/pasted", "https://example.com/bare"];
        for (const href of hrefs) {
          await waitDescriptionContains(seed, projectId, pageId, session, href);
        }
        // A slow or reordered autosave can still be landing after the last
        // mutation; settle the record, and push one healing save when
        // editor-held content is missing from the settled state.
        let settled = await waitDescriptionSettled(seed, projectId, pageId, session);
        if (!hrefs.every((href) => settled.includes(href))) {
          await driver.pagesBlocksEditorClickEnd();
          await driver.pagesBlocksEditorType("x");
          await driver.pagesBlocksEditorPressKey("Backspace");
          for (const href of hrefs) {
            await waitDescriptionContains(seed, projectId, pageId, session, href);
          }
          settled = await waitDescriptionSettled(seed, projectId, pageId, session);
        }
        for (const href of hrefs) expect(settled).toContain(href);
        await driver.pagesBlocksReload();
        expect(await driver.pagesBlocksLinkHref()).toBe("https://example.com/edited");
        const html = await driver.pagesBlocksEditorHtml();
        expect(html).toContain('href="https://example.com/pasted"');
        expect(html).toContain('href="https://example.com/bare"');
      });
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-044"], "options-menu entry copies the body as markup with a success notice"),
  { tag: specTags(["PAGE-044"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "markup", session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("markup")} page`);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await serverUpdatePageDescription(
        seed.workspaceSlug,
        projectId,
        pageId,
        session,
        "<h1>Copy Title</h1><p>copy body text</p>",
        null
      );
      await driver.pagesBlocksOpenDetail(seed.workspaceSlug, projectId, pageId);

      await test.step("the seeded body loads before copying", async () => {
        // The copy converts whatever the editor holds; clicking before the
        // fetch lands copies an empty body while the toast still fires.
        await waitEditorContains(driver, ["Copy Title", "copy body text"]);
      });

      await test.step("the options menu carries the markup-copy entry", async () => {
        expect(await driver.pagesBlocksOptionsItems()).toContain("Copy markdown");
      });

      let markup = "";
      await test.step("copying writes markup and raises the success notice", async () => {
        // Old bug NEWFRONT-373: the store's editor ref can go stale on mount
        // (helpers built before the editor instance exists), and the entry
        // then toasts success while writing nothing, for the whole page
        // lifetime. Locking and unlocking re-syncs the ref, so the scenario
        // recovers that way once before failing loudly.
        let copied = false;
        for (let attempt = 0; attempt < 2 && !copied; attempt++) {
          if (attempt === 1) await driver.pagesBlocksHeaderLockCycle();
          await driver.pagesBlocksOptionsCopyMarkdown();
          expect(await driver.pagesBlocksToastText()).toContain("Markdown copied to clipboard.");
          try {
            await expect
              .poll(() => driver.pagesBlocksClipboardText(), { timeout: 15_000 })
              .toBe("# Copy Title\n\ncopy body text\n");
            copied = true;
          } catch {
            if (attempt === 1) throw new Error("[parity] clipboard never carried the markup copy.");
          }
        }
        markup = await driver.pagesBlocksClipboardText();
      });

      await test.step("pasting yields the markup form", async () => {
        const pastePageId = await serverCreatePage(
          seed.workspaceSlug,
          projectId,
          session,
          `${tag("markup-paste")} page`
        );
        await driver.pagesBlocksOpenDetail(seed.workspaceSlug, projectId, pastePageId);
        await driver.pagesBlocksEditorClickEnd();
        await driver.pagesBlocksEditorPasteText(markup);
        const html = await driver.pagesBlocksEditorHtml();
        expect(html).toContain("<h1");
        expect(html).toContain("Copy Title");
        expect(html).toContain("copy body text");
      });
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);
