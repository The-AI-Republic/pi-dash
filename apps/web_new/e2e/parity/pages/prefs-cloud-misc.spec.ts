// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-320): pages display preferences and the
// toolbar key map, the cloud assistant menu and cloud-only stub slots,
// desktop parity, and the load-bearing absences the new app must
// preserve. Rows: PAGE-072-073, PAGE-075-078, PAGE-081-083.
import { test, expect } from "../fixtures";
import {
  pagesProbeAssistantRoot,
  pagesProbeEditorTask,
  parityProjectIdentifier,
  serverCleanupProject,
  serverCreatePage,
  serverCreateProjectWithFlags,
  serverPageRecord,
  serverUpdatePageDescription,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParitySeedFacts } from "../drivers/parity-driver";

/** Unique area tag so scratch projects and page names never collide. */
function tag(area: string): string {
  return `NF320 ${area} ${Date.now()}`;
}

/** Visible text of saved HTML: the editor's own autosave can normalize an
 * empty document's markup in the window (block classes, extra empty
 * blocks), so "saved content untouched" compares text, not bytes. */
function textOf(html: string): string {
  return html
    .replace(/<[^>]*>/g, " ")
    .replace(/&nbsp;/g, " ")
    .replace(/\s+/g, " ")
    .trim();
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
    parityProjectIdentifier("N320"),
    {},
    session
  );
  return { projectId, name };
}

/** Advertised key map the toolbar tooltips must show (PAGE-073). */
const TOOLTIP_KEY_MAP: Array<{ label: string; keys: string }> = [
  { label: "Bold", keys: "Cmd + B" },
  { label: "Italic", keys: "Cmd + I" },
  { label: "Underline", keys: "Cmd + U" },
  { label: "Strikethrough", keys: "Cmd + Shift + S" },
  { label: "Left align", keys: "Cmd + Shift + L" },
  { label: "Center align", keys: "Cmd + Shift + E" },
  { label: "Right align", keys: "Cmd + Shift + R" },
  { label: "Numbered list", keys: "Cmd + Shift + 7" },
  { label: "Bulleted list", keys: "Cmd + Shift + 8" },
  { label: "To-do list", keys: "Cmd + Shift + 9" },
];

/** Community slash catalog in display order (PAGE-078 cross-check). */
const SLASH_CATALOG: string[] = [
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
];

test(
  specTitle(["PAGE-072"], "display toggles reflow immediately, persist across reload, and write no server state"),
  { tag: specTags(["PAGE-072"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "prefs", session);
    const pageName = `${tag("prefs")} page`;
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, pageName);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.pagesPrefsOpenDetail(seed.workspaceSlug, projectId, pageId);
      const before = await serverPageRecord(seed.workspaceSlug, projectId, pageId, session);

      await test.step("the options menu offers the two display toggles", async () => {
        await driver.pagesPrefsOptionsOpen();
        const items = await driver.pagesPrefsOptionsItems();
        expect(items.some((item) => item.includes("Full width"))).toBe(true);
        expect(items.some((item) => item.includes("Sticky toolbar"))).toBe(true);
        await driver.pagesKeysPress("Escape");
      });

      await test.step("toggling reflows immediately and persists locally", async () => {
        expect(await driver.pagesPrefsToolbarWide()).toBe(false);
        expect(await driver.pagesPrefsCanvasMaxWidth()).toBe("720px");
        expect(await driver.pagesPrefsToolbarVisible()).toBe(true);

        await driver.pagesPrefsSpyPageWrites();
        await driver.pagesPrefsOptionsOpen();
        await driver.pagesPrefsOptionsToggle("Full width");
        expect(await driver.pagesPrefsToolbarWide()).toBe(true);
        expect(await driver.pagesPrefsCanvasMaxWidth()).toBe("1152px");
        expect((await driver.pagesPrefsLocalConfig())?.fullWidth).toBe(true);

        await driver.pagesPrefsOptionsOpen();
        await driver.pagesPrefsOptionsToggle("Sticky toolbar");
        expect(await driver.pagesPrefsToolbarVisible()).toBe(false);
        expect((await driver.pagesPrefsLocalConfig())?.stickyToolbar).toBe(false);

        await driver.pagesPrefsOptionsOpen();
        await driver.pagesPrefsOptionsToggle("Sticky toolbar");
        expect(await driver.pagesPrefsToolbarVisible()).toBe(true);
      });

      await test.step("the preference moves survive reload and write no server state", async () => {
        // The preference moves themselves emit no server write: any page
        // write in the window (the content autosave can fire) must carry
        // only content keys, never preference keys.
        const payloads = await driver.pagesPrefsPageWritePayloads();
        for (const payload of payloads) {
          expect(payload).not.toMatch(/full_width|font_size|font_style|sticky_toolbar|pages_config/);
        }
        await driver.pagesPrefsReload();
        // Stored prefs apply a beat after the editor mounts, so poll.
        await expect.poll(() => driver.pagesPrefsToolbarWide(), { timeout: 30_000 }).toBe(true);
        await expect.poll(() => driver.pagesPrefsCanvasMaxWidth(), { timeout: 30_000 }).toBe("1152px");
        await expect.poll(() => driver.pagesPrefsToolbarVisible(), { timeout: 30_000 }).toBe(true);
        expect(await driver.pagesPrefsLocalConfig()).toEqual({
          fullWidth: true,
          fontSize: "large-font",
          fontStyle: "sans-serif",
          stickyToolbar: true,
        });

        const after = await serverPageRecord(seed.workspaceSlug, projectId, pageId, session);
        expect(after.name).toBe(before.name);
        expect(textOf(after.descriptionHtml)).toBe(textOf(before.descriptionHtml));
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-072"], "type size and face follow the persisted config without touching saved content"),
  { tag: specTags(["PAGE-072"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "fonts", session);
    const pageName = `${tag("fonts")} page`;
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, pageName);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.pagesPrefsOpenDetail(seed.workspaceSlug, projectId, pageId);
      const before = await serverPageRecord(seed.workspaceSlug, projectId, pageId, session);

      await test.step("a compact serif config reflows title and body after reload", async () => {
        await driver.pagesPrefsSpyPageWrites();
        await driver.pagesPrefsSeedLocalConfig({
          fullWidth: false,
          fontSize: "small-font",
          fontStyle: "serif",
          stickyToolbar: true,
        });
        await driver.pagesPrefsReload();
        expect(await driver.pagesPrefsTitleClass()).toContain("small-font");
        const editorClass = await driver.pagesPrefsEditorClass();
        expect(editorClass).toContain("small-font");
        expect(editorClass).toContain("serif");
      });

      await test.step("the community build offers no font switcher of its own", async () => {
        await driver.pagesPrefsOptionsOpen();
        const items = await driver.pagesPrefsOptionsItems();
        expect(items.some((item) => /font|typeface|typography/i.test(item))).toBe(false);
        await driver.pagesKeysPress("Escape");
      });

      await test.step("saved content is untouched and defaults restore", async () => {
        const payloads = await driver.pagesPrefsPageWritePayloads();
        for (const payload of payloads) {
          expect(payload).not.toMatch(/full_width|font_size|font_style|sticky_toolbar|pages_config/);
        }
        const after = await serverPageRecord(seed.workspaceSlug, projectId, pageId, session);
        expect(after.name).toBe(before.name);
        expect(textOf(after.descriptionHtml)).toBe(textOf(before.descriptionHtml));

        await driver.pagesPrefsSeedLocalConfig({
          fullWidth: false,
          fontSize: "large-font",
          fontStyle: "sans-serif",
          stickyToolbar: true,
        });
        await driver.pagesPrefsReload();
        expect(await driver.pagesPrefsTitleClass()).toContain("large-font");
        const editorClass = await driver.pagesPrefsEditorClass();
        expect(editorClass).toContain("large-font");
        expect(editorClass).toContain("sans-serif");
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-073"], "toolbar tooltips advertise the ten-command key map"),
  { tag: specTags(["PAGE-073"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "tooltips", session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("tooltips")} page`);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.pagesPrefsOpenDetail(seed.workspaceSlug, projectId, pageId);

      const tooltips = await driver.pagesKeysToolbarTooltips();
      for (const expected of TOOLTIP_KEY_MAP) {
        const found = tooltips.find((tip) => tip.label === expected.label);
        expect(found, `tooltip for ${expected.label}`).toBeDefined();
        expect(found?.keys).toBe(expected.keys);
      }

      const hinted = new Set(TOOLTIP_KEY_MAP.map((entry) => entry.label));
      for (const tip of tooltips) {
        if (!hinted.has(tip.label)) expect(tip.keys, `unhinted ${tip.label}`).toBe("");
      }
      expect(await driver.pagesKeysShortcutReferenceVisible()).toBe(false);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-073"], "each hinted combination fires its command with the editor focused"),
  { tag: specTags(["PAGE-073"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "combos", session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("combos")} page`);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.pagesPrefsOpenDetail(seed.workspaceSlug, projectId, pageId);

      const fire = async (chord: string, marker: RegExp) => {
        await driver.pagesPrefsEditorClear();
        await driver.pagesPrefsEditorClickEnd();
        await driver.pagesPrefsEditorType("combo probe");
        await driver.pagesPrefsEditorSelectAll();
        await driver.pagesKeysPress(chord);
        await expect.poll(() => driver.pagesPrefsEditorHtml(), { timeout: 30_000 }).toMatch(marker);
      };

      await test.step("the four basic marks toggle", async () => {
        await fire("ControlOrMeta+b", /<strong>/);
        await fire("ControlOrMeta+i", /<em>/);
        await fire("ControlOrMeta+u", /<u>/);
        await fire("ControlOrMeta+Shift+s", /<s>|<strike>|<del>/);
      });

      await test.step("the three alignments apply", async () => {
        await fire("ControlOrMeta+Shift+l", /text-align:\s*left/);
        await fire("ControlOrMeta+Shift+e", /text-align:\s*center/);
        await fire("ControlOrMeta+Shift+r", /text-align:\s*right/);
      });

      await test.step("the three list kinds wrap the paragraph", async () => {
        await fire("ControlOrMeta+Shift+7", /<ol/);
        await fire("ControlOrMeta+Shift+8", /<ul(?![^>]*task-list)/);
        await fire("ControlOrMeta+Shift+9", /task-?list|todo/i);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-073"], "save gesture, title Enter, and search Escape behave; no cheat sheet exists"),
  { tag: specTags(["PAGE-073"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "gestures", session);
    const firstTitle = `${tag("gestures")} Alpha`;
    const secondTitle = `${tag("gestures")} Beta`;
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, firstTitle);
    await serverCreatePage(seed.workspaceSlug, projectId, session, secondTitle);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("the save gesture is captured without leaving the page", async () => {
        await driver.pagesPrefsOpenDetail(seed.workspaceSlug, projectId, pageId);
        await driver.pagesPrefsEditorClickEnd();
        await driver.pagesPrefsEditorType("gesture probe");
        await driver.pagesKeysPress("ControlOrMeta+s");
        await driver.pagesKeysPress("Escape");
        expect(await driver.pagesPrefsEditorText()).toContain("gesture probe");
        expect(await driver.pagesKeysShortcutReferenceVisible()).toBe(false);
      });

      await test.step("Enter in the title moves focus into the body", async () => {
        await driver.pagesPrefsTitleFocus();
        expect(await driver.pagesPrefsFocusedRegion()).toBe("title");
        await driver.pagesPrefsTitlePressEnter();
        await expect.poll(() => driver.pagesPrefsFocusedRegion(), { timeout: 30_000 }).toBe("body");
      });

      await test.step("Escape clears the list search first and collapses it when empty", async () => {
        await driver.pagesPrefsOpenList(seed.workspaceSlug, projectId);
        await driver.pagesKeysSearchOpen();
        await driver.pagesKeysSearchFill("Alpha");
        await expect.poll(() => driver.pagesKeysListRowTitles(), { timeout: 30_000 }).toEqual([firstTitle]);
        await driver.pagesKeysSearchPressEscape();
        expect(await driver.pagesKeysSearchValue()).toBe("");
        expect(await driver.pagesKeysSearchExpanded()).toBe(true);
        await expect
          .poll(() => driver.pagesKeysListRowTitles(), { timeout: 30_000 })
          .toEqual(expect.arrayContaining([firstTitle, secondTitle]));
        await driver.pagesKeysSearchPressEscape();
        // The collapse animates (transition-[width]), so poll for it.
        await expect.poll(() => driver.pagesKeysSearchExpanded(), { timeout: 30_000 }).toBe(false);
        expect(await driver.pagesKeysShortcutReferenceVisible()).toBe(false);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-075"], "community build carries no assistant surface; the task endpoints answer absent"),
  { tag: specTags(["PAGE-075"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "assist", session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("assist")} page`);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.pagesPrefsOpenDetail(seed.workspaceSlug, projectId, pageId);
      await driver.pagesPrefsEditorClickEnd();
      await driver.pagesPrefsEditorType("assistant probe text");

      await test.step("no AI handle or menu renders anywhere on the detail", async () => {
        expect(await driver.pagesAssistAiHandleVisible()).toBe(false);
        expect(await driver.pagesAssistAiMenuVisible()).toBe(false);
      });

      await test.step("the workspace task endpoints answer unreachable", async () => {
        // The editor-task route was never registered server-side; the
        // assistant root is routed but refuses without a configured
        // model (400 "LLM provider API key and model are required").
        expect(await pagesProbeEditorTask(seed.workspaceSlug, session)).toBe(404);
        expect(await pagesProbeAssistantRoot(seed.workspaceSlug, session)).toBe(400);
      });

      await test.step("the page itself stays intact", async () => {
        const record = await serverPageRecord(seed.workspaceSlug, projectId, pageId, session);
        expect(record.id).toBe(pageId);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-076"], "header share slot renders nothing in the community build"),
  { tag: specTags(["PAGE-076"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "share", session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("share")} page`);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.pagesPrefsOpenDetail(seed.workspaceSlug, projectId, pageId);

      const names = await driver.pagesCloudHeaderActionNames();
      expect(names.some((name) => /share/i.test(name))).toBe(false);
      expect(names.some((name) => /copy link/i.test(name))).toBe(true);
      expect(names.some((name) => /favorit/i.test(name))).toBe(true);

      const record = await serverPageRecord(seed.workspaceSlug, projectId, pageId, session);
      expect(record.id).toBe(pageId);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-077"], "header collaborator strip renders nothing; the server carries no presence roster"),
  { tag: specTags(["PAGE-077"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "roster", session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("roster")} page`);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.pagesPrefsOpenDetail(seed.workspaceSlug, projectId, pageId);

      expect(await driver.pagesCloudToolbarEndNames()).toEqual(["(icon:panel-right)"]);

      const record = await serverPageRecord(seed.workspaceSlug, projectId, pageId, session);
      expect(record.id).toBe(pageId);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-078"], "extension slots and cloud registries contribute nothing in the community build"),
  { tag: specTags(["PAGE-078"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "slots", session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("slots")} page`);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.pagesPrefsOpenDetail(seed.workspaceSlug, projectId, pageId);

      await test.step("the header carries no further action buttons", async () => {
        const names = await driver.pagesCloudHeaderActionNames();
        expect(names.length).toBeLessThanOrEqual(5);
        expect(names.some((name) => /share|extension|plugin|extra/i.test(name))).toBe(false);
      });

      await test.step("the pane offers exactly the community tabs", async () => {
        await driver.pagesCloudPaneOpen();
        expect(await driver.pagesCloudPaneTabNames()).toEqual(["Outline", "Info", "Assets"]);
      });

      await test.step("the slash catalog equals the community set", async () => {
        expect(await driver.pagesCloudSlashEntries()).toEqual(SLASH_CATALOG);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-081"], "desktop gets the same pages experience as web with no gated variants"),
  { tag: specTags(["PAGE-081"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "desk", session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("desk")} page`);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("the context reads as a desktop platform", async () => {
        expect(await driver.pagesDeskIsMobile()).toBe(false);
        expect(["Windows", "MacOS", "Linux"]).toContain(await driver.pagesDeskPlatform());
      });

      await test.step("list and detail carry the full ungated experience", async () => {
        await driver.pagesPrefsOpenList(seed.workspaceSlug, projectId);
        expect(await driver.pagesNegListTabNames()).toEqual(["Public", "Private", "Archived"]);

        await driver.pagesPrefsOpenDetail(seed.workspaceSlug, projectId, pageId);
        expect(await driver.pagesPrefsToolbarVisible()).toBe(true);
        await driver.pagesPrefsOptionsOpen();
        expect((await driver.pagesPrefsOptionsItems()).length).toBeGreaterThan(0);
        await driver.pagesKeysPress("Escape");
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-082"], "no cover affordance appears anywhere in list or detail"),
  { tag: specTags(["PAGE-082"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "cover", session);
    const title = `${tag("cover")} page`;
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, title);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("list rows carry no cover", async () => {
        await driver.pagesPrefsOpenList(seed.workspaceSlug, projectId);
        await expect.poll(() => driver.pagesKeysListRowTitles(), { timeout: 30_000 }).toContain(title);
        expect(await driver.pagesNegCoverPresent()).toBe(false);
      });

      await test.step("detail carries a mark but no cover path", async () => {
        await driver.pagesPrefsOpenDetail(seed.workspaceSlug, projectId, pageId);
        expect(await driver.pagesNegMarkPresent()).toBe(true);
        expect(await driver.pagesNegCoverPresent()).toBe(false);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-083"], "favorites tab, row export, shortcut reference, and conflict dialog stay absent"),
  { tag: specTags(["PAGE-083"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "neg", session);
    const title = `${tag("neg")} page`;
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, title);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("the list offers no favorites tab and rows offer no export", async () => {
        await driver.pagesPrefsOpenList(seed.workspaceSlug, projectId);
        expect(await driver.pagesNegListTabNames()).toEqual(["Public", "Private", "Archived"]);
        const items = await driver.pagesNegRowMenuItems(title);
        // The six-entry oracle prefix is pinned on both apps. D17 appends
        // Add sub-page and Move to… on web_new only (PAGE-084/087), so the
        // suffix is part of PAGE-083's expected order there.
        expect(items.slice(0, 6)).toEqual([
          "Open in new tab",
          "Copy link",
          "Make a copy",
          "Lock",
          "Make private",
          "Archive",
        ]);
        expect(items.slice(6)).toEqual(driver.target === "web_new" ? ["Add sub-page", "Move to…"] : []);
        expect(items).toContain("Copy link");
        expect(items).toContain("Make a copy");
        expect(items.some((item) => /export/i.test(item))).toBe(false);
        expect(await driver.pagesKeysShortcutReferenceVisible()).toBe(false);
      });

      await test.step("export lives in the detail menu instead", async () => {
        await driver.pagesPrefsOpenDetail(seed.workspaceSlug, projectId, pageId);
        await driver.pagesPrefsOptionsOpen();
        const items = await driver.pagesPrefsOptionsItems();
        expect(items.some((item) => /export/i.test(item))).toBe(true);
        await driver.pagesKeysPress("Escape");
      });

      await test.step("concurrent edits converge silently with no conflict dialog", async () => {
        await serverUpdatePageDescription(seed.workspaceSlug, projectId, pageId, session, "<p>remote edit</p>");
        await driver.pagesPrefsEditorClickEnd();
        await driver.pagesPrefsEditorType(" local edit");
        expect(await driver.pagesNegConflictDialogVisible()).toBe(false);
        expect(await driver.pagesKeysShortcutReferenceVisible()).toBe(false);
        const record = await serverPageRecord(seed.workspaceSlug, projectId, pageId, session);
        expect(record.id).toBe(pageId);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
