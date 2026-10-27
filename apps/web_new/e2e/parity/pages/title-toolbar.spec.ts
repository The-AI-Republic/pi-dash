// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-315): page-detail title editing, the mark
// picker, and the sticky formatting toolbar — typography, text marks,
// alignment, lists, quote/code, tables, images, bar visibility, colors.
// Rows: PAGE-026–PAGE-036, PAGE-091.
//
// Behavior pinned live against the old app (prod oracle, no live server):
// - The title is a single-h1 TipTap editor with no length cap and no count
//   (the capped TextArea in the old tree is dead code); it PATCHes the name
//   immediately, outside the body save path. Enter moves focus to the body.
// - The mark picker is set-only: emoji/icon tabs persist, but nothing
//   clears a mark (re-pick is a no-op, null PATCH answers 400).
// - Loads whose page data arrives after the first editor creation bind the
//   toolbar to a dead instance (typing works, every command no-ops): the
//   old editor recreates on the `editable` flip but the toolbar never
//   rebinds. pagesToolbarSettle verifies a bold round-trip, reloading
//   until the bind proves live.
import { test, expect } from "../fixtures";
import { writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  pagesLiveSetPageLock,
  pagesTitlePatchRecordLenient,
  pagesTitlePollName,
  pagesTitleReadRecord,
  pagesToolbarAssetBytes,
  pagesToolbarPollDescriptionHtml,
  parityProjectIdentifier,
  serverCleanupProject,
  serverCreatePage,
  serverCreateProjectWithFlags,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";

/**
 * Unique area tag so scratch projects and page names never collide.
 * Hyphens become spaces: project names reject special characters.
 */
function tag(area: string): string {
  return `NF315 ${area.replace(/-/g, " ")} ${Date.now()}`;
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
    parityProjectIdentifier("N315"),
    {},
    session
  );
  return { projectId, name };
}

/** Sign in, create a page, and open its detail view. */
async function openFreshPage(
  driver: ParityDriver,
  seed: ParitySeedFacts,
  session: string,
  area: string,
  pageName?: string
): Promise<{ projectId: string; pageId: string; pageName: string }> {
  const { projectId } = await scratchProject(seed, area, session);
  const name = pageName ?? `${tag(area)} page`;
  const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, name);
  await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
  await driver.pagesLiveOpenPageDetail(seed.workspaceSlug, projectId, pageId);
  return { projectId, pageId, pageName: name };
}

/** Poll the page record until its mark matches `want` (JSON substring). */
async function pollLogoProps(
  seed: ParitySeedFacts,
  projectId: string,
  pageId: string,
  session: string,
  want: string,
  timeoutMs = 60_000
): Promise<unknown> {
  let logoProps: unknown = null;
  await expect
    .poll(
      async () => {
        logoProps = (await pagesTitleReadRecord(seed.workspaceSlug, projectId, pageId, session)).logoProps;
        return JSON.stringify(logoProps);
      },
      { timeout: timeoutMs }
    )
    .toContain(want);
  return logoProps;
}

/** 1px PNG upload fixture (written per use; never committed). */
function pngPath(area: string): string {
  const file = join(tmpdir(), `parity315-${area}-${Date.now()}-${Math.floor(Math.random() * 1e6)}.png`);
  writeFileSync(
    file,
    Buffer.from(
      "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==",
      "base64"
    )
  );
  return file;
}

/**
 * Click a toggle button and verify its effect, re-selecting and re-clicking
 * once when the application misses (a layout shift can send a click past
 * its button on a loaded host). Late arrivals count: the state is re-read
 * before the retry so a slow application is never toggled back off.
 */
async function expectToggle(
  driver: ParityDriver,
  index: number,
  marker: string | RegExp,
  present: boolean
): Promise<void> {
  const matches = (html: string): boolean => (typeof marker === "string" ? html.includes(marker) : marker.test(html));
  const pollState = async (): Promise<void> => {
    if (typeof marker === "string") {
      if (present) {
        await expect.poll(() => driver.pagesToolbarBodyHtml(), { timeout: 10_000 }).toContain(marker);
      } else {
        await expect.poll(() => driver.pagesToolbarBodyHtml(), { timeout: 10_000 }).not.toContain(marker);
      }
    } else if (present) {
      await expect.poll(() => driver.pagesToolbarBodyHtml(), { timeout: 10_000 }).toMatch(marker);
    } else {
      throw new Error("[parity] negative regexp toggles are not used.");
    }
  };
  for (let attempt = 0; attempt < 2; attempt += 1) {
    await driver.pagesToolbarSelectAll();
    await driver.pagesToolbarClick(index);
    try {
      await pollState();
      return;
    } catch {
      if (matches(await driver.pagesToolbarBodyHtml())) {
        if (present) return;
      } else if (!present) {
        return;
      }
      // Otherwise reselect and re-click once.
    }
  }
  throw new Error(`[parity] toolbar toggle ${index} never settled ${String(marker)}.`);
}

/**
 * Apply a color with one retry: opening the popover occasionally drops the
 * editor selection on a loaded host, so a missed application re-selects
 * and re-picks instead of failing the scenario.
 */
async function applyColor(
  driver: ParityDriver,
  kind: "text" | "background",
  index: number,
  needle: string
): Promise<void> {
  for (let attempt = 0; attempt < 2; attempt += 1) {
    await driver.pagesToolbarSelectAll();
    await driver.pagesToolbarColorOpen();
    await driver.pagesToolbarColorPick(kind, index);
    try {
      await expect.poll(() => driver.pagesToolbarBodyHtml(), { timeout: 10_000 }).toContain(needle);
      return;
    } catch {
      // Retry from a fresh selection; the next select-all closes the popover.
    }
  }
  throw new Error(`[parity] color ${kind}[${index}] never applied.`);
}

/**
 * Clear a color with one retry: like application, the clear click can land
 * on a dropped selection when opening the popover under load, so a missed
 * clear re-selects and re-clears instead of failing the scenario.
 */
async function clearColor(driver: ParityDriver, kind: "text" | "background", needle: string): Promise<void> {
  for (let attempt = 0; attempt < 2; attempt += 1) {
    await driver.pagesToolbarSelectAll();
    await driver.pagesToolbarColorOpen();
    await driver.pagesToolbarColorClear(kind);
    try {
      await expect.poll(() => driver.pagesToolbarBodyHtml(), { timeout: 10_000 }).not.toContain(needle);
      return;
    } catch {
      // Retry from a fresh selection; the next select-all closes the popover.
    }
  }
  throw new Error(`[parity] color ${kind} never cleared.`);
}

test(
  specTitle(["PAGE-026"], "title edits save immediately outside the body save path"),
  { tag: specTags(["PAGE-026"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "title-save", session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("title-save")} page`);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.pagesLiveOpenPageDetail(seed.workspaceSlug, projectId, pageId);

      const before = await pagesTitleReadRecord(seed.workspaceSlug, projectId, pageId, session);
      // Slow typing: per-keystroke title PATCHes race at spec speed and can
      // keep a prefix server-side (NEWFRONT-358); 200ms exceeds a round-trip.
      const typed = `${tag("title-save")} renamed`;
      await driver.pagesTitleSetTitle(typed, 200);
      expect(await driver.pagesLiveTitleText()).toBe(typed);

      await test.step("server name follows typing while the body stays untouched", async () => {
        expect(await pagesTitlePollName(seed.workspaceSlug, projectId, pageId, session, typed)).toBe(typed);
        const record = await pagesTitleReadRecord(seed.workspaceSlug, projectId, pageId, session);
        // Byte equality is too strict: opening the editor normalizes the
        // stored empty paragraph and the fallback save persists that. The
        // pin is semantic — no title text reaches the body, still empty.
        const textOf = (html: string): string => html.replace(/<[^>]*>/g, "").trim();
        expect(textOf(before.descriptionHtml)).toBe("");
        expect(textOf(record.descriptionHtml)).toBe("");
        expect(record.descriptionHtml).not.toContain(typed);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-026"], "Enter in the title jumps to the top of the body"),
  { tag: specTags(["PAGE-026"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await openFreshPage(driver, seed, session, "title-enter");
    try {
      const titleBefore = await driver.pagesLiveTitleText();
      await driver.pagesLiveTypeInBody("first line");
      await driver.pagesToolbarPressKey("Enter");
      await driver.pagesToolbarTypeText("second line");

      await driver.pagesTitlePressEnter();
      // The focus handoff is async: poll it rather than racing a single read.
      await expect.poll(() => driver.pagesTitleFocusInBody(), { timeout: 15_000 }).toBe(true);
      expect(await driver.pagesTitleBodyCaretAtTop()).toBe(true);
      // No newline leaks into the title itself.
      expect(await driver.pagesLiveTitleText()).toBe(titleBefore);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-026"], "bug: NEWFRONT-354 over-255 titles persist and no count badge renders"),
  { tag: specTags(["PAGE-026"]) },
  async ({ driver, seed }, testInfo) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId, pageId } = await openFreshPage(driver, seed, session, "title-cap");
    try {
      // Intended: input past 255 characters is blocked with a live count
      // while focused (the create dialog enforces it). Observed: the live
      // h1 title editor accepts and persists over-limit input with no
      // badge, focused or blurred.
      const long = "x".repeat(300);
      await driver.pagesTitleSetTitle(long, 150);
      // Bug NEWFRONT-354: the oracle pins the old over-limit persistence;
      // the new app implements the intended fix instead (Parity wiki "Old
      // bugs": web_new may implement the fix; spec §5.1 pins the 255 cap
      // with a focused-only live count).
      if (testInfo.project.name === "new") {
        expect(
          await pagesTitlePollName(seed.workspaceSlug, projectId, pageId, session, long.slice(0, 255))
        ).toHaveLength(255);
      } else {
        expect(await pagesTitlePollName(seed.workspaceSlug, projectId, pageId, session, long)).toHaveLength(300);
      }

      await driver.pagesTitleFocus();
      expect(await driver.pagesTitleIsFocused()).toBe(true);
      if (testInfo.project.name === "new") {
        expect(await driver.pagesTitleCountBadge()).toBe("255/255");
      } else {
        expect(await driver.pagesTitleCountBadge()).toBe(null);
      }

      await driver.pagesLiveTypeInBody("z");
      expect(await driver.pagesTitleCountBadge()).toBe(null);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-026"], "read-only pages show static title text with an Untitled fallback"),
  { tag: specTags(["PAGE-026"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "title-ro", session);
    const named = `${tag("title-ro")} named`;
    const namedId = await serverCreatePage(seed.workspaceSlug, projectId, session, named);
    const emptyId = await serverCreatePage(seed.workspaceSlug, projectId, session, "");
    try {
      await pagesLiveSetPageLock(seed.workspaceSlug, projectId, namedId, true, session);
      await pagesLiveSetPageLock(seed.workspaceSlug, projectId, emptyId, true, session);
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await driver.pagesLiveOpenPageDetail(seed.workspaceSlug, projectId, namedId);
      await expect.poll(() => driver.pagesTitleEditable(), { timeout: 30_000 }).toBe(false);
      await expect.poll(() => driver.pagesLiveTitleText(), { timeout: 30_000 }).toBe(named);

      await driver.pagesLiveOpenPageDetail(seed.workspaceSlug, projectId, emptyId);
      await expect.poll(() => driver.pagesTitleEditable(), { timeout: 30_000 }).toBe(false);
      await expect.poll(() => driver.pagesLiveTitleText(), { timeout: 30_000 }).toBe("");
      await expect.poll(() => driver.pagesTitleEmptyFallback(), { timeout: 30_000 }).toContain("Untitled");
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-027"], "mark affordance reveals on hover and pins while the title is empty"),
  { tag: specTags(["PAGE-027"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "mark-afford", session);
    const namedId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("mark-afford")} named`);
    const emptyId = await serverCreatePage(seed.workspaceSlug, projectId, session, "");
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await driver.pagesLiveOpenPageDetail(seed.workspaceSlug, projectId, namedId);
      expect(await driver.pagesMarkAffordanceOpacity()).toBe("0");
      await driver.pagesMarkHoverHeader();
      await expect.poll(() => driver.pagesMarkAffordanceOpacity(), { timeout: 10_000 }).toBe("1");

      await driver.pagesLiveOpenPageDetail(seed.workspaceSlug, projectId, emptyId);
      expect(await driver.pagesMarkAffordanceOpacity()).toBe("1");
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-027"], "emoji and icon picks persist across reload"),
  { tag: specTags(["PAGE-027"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId, pageId } = await openFreshPage(driver, seed, session, "mark-pick");
    try {
      await driver.pagesMarkOpenPicker();
      expect(await driver.pagesMarkTabs()).toEqual(["Emoji", "Icon"]);

      await driver.pagesMarkSearchPick("🚀", "rocket");
      const picked = await pollLogoProps(seed, projectId, pageId, session, '"in_use":"emoji"');
      expect(JSON.stringify(picked)).toContain("128640");

      // The mark survives a reload and renders in the header.
      await driver.pagesLiveOpenPageDetail(seed.workspaceSlug, projectId, pageId);
      expect(await driver.pagesMarkCurrent()).toEqual({ kind: "emoji", label: "🚀" });
      const reloaded = await pagesTitleReadRecord(seed.workspaceSlug, projectId, pageId, session);
      expect(JSON.stringify(reloaded.logoProps)).toContain('"in_use":"emoji"');

      // Changing the mark persists too.
      await driver.pagesMarkOpenPicker();
      await driver.pagesMarkPickIcon(1);
      const changed = await pollLogoProps(seed, projectId, pageId, session, '"in_use":"icon"');
      expect(JSON.stringify(changed)).toContain('"name"');
      const current = await driver.pagesMarkCurrent();
      expect(current?.kind).toBe("icon");
      expect(current?.label.length).toBeGreaterThan(0);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-027"], "the chosen mark updates list, breadcrumb, and switcher glyphs"),
  { tag: specTags(["PAGE-027"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId, pageId, pageName } = await openFreshPage(driver, seed, session, "mark-glyph");
    try {
      await driver.pagesMarkOpenPicker();
      await driver.pagesMarkSearchPick("🚀", "rocket");
      await pollLogoProps(seed, projectId, pageId, session, '"in_use":"emoji"');

      const breadcrumb = await driver.pagesMarkBreadcrumbGlyph(pageName);
      expect(breadcrumb).toContain("🚀");
      expect(breadcrumb).toContain(pageName);

      const switcher = await driver.pagesMarkSwitcherGlyph(pageName);
      expect(switcher).toContain("🚀");
      expect(switcher).toContain(pageName);

      // List navigation goes last: it leaves the detail view.
      const row = await driver.pagesMarkListGlyph(seed.workspaceSlug, projectId, pageName);
      expect(row).toContain("🚀");
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-027"], "bug: NEWFRONT-355 marks cannot be cleared and the API rejects null with 400"),
  { tag: specTags(["PAGE-027"]) },
  async ({ driver, seed }, testInfo) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId, pageId } = await openFreshPage(driver, seed, session, "mark-clear");
    try {
      // Intended: picking, changing, or clearing a mark persists. Observed:
      // the picker offers no remove control, re-picking is a no-op, and a
      // null mark PATCH answers 400 with the mark intact.
      await driver.pagesMarkOpenPicker();
      expect(await driver.pagesMarkClearControl()).toBe(false);
      await driver.pagesToolbarPressKey("Escape");

      await driver.pagesMarkOpenPicker();
      await driver.pagesMarkPickIcon(1);
      const picked = await pollLogoProps(seed, projectId, pageId, session, '"in_use":"icon"');

      await driver.pagesMarkOpenPicker();
      // Bug NEWFRONT-355: the oracle pins the old picker's missing remove
      // control; the new app implements the intended fix instead (Parity
      // wiki "Old bugs": web_new may implement the fix; spec §5.2 pins the
      // picker's clear action on a marked page).
      if (testInfo.project.name === "new") {
        expect(await driver.pagesMarkClearControl()).toBe(true);
      } else {
        expect(await driver.pagesMarkClearControl()).toBe(false);
      }
      await driver.pagesMarkPickIcon(1);
      await expect
        .poll(
          async () =>
            JSON.stringify((await pagesTitleReadRecord(seed.workspaceSlug, projectId, pageId, session)).logoProps),
          { timeout: 15_000 }
        )
        .toBe(JSON.stringify(picked));

      const refusal = await pagesTitlePatchRecordLenient(seed.workspaceSlug, projectId, pageId, session, {
        logo_props: null,
      });
      expect(refusal.status).toBe(400);
      const survived = await pagesTitleReadRecord(seed.workspaceSlug, projectId, pageId, session);
      expect(JSON.stringify(survived.logoProps)).toBe(JSON.stringify(picked));
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-027"], "the mark surface is disabled for read-only viewers"),
  { tag: specTags(["PAGE-027"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "mark-ro", session);
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("mark-ro")} page`);
    try {
      await pagesLiveSetPageLock(seed.workspaceSlug, projectId, pageId, true, session);
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.pagesLiveOpenPageDetail(seed.workspaceSlug, projectId, pageId);

      expect(await driver.pagesMarkPickerDisabled()).toBe(true);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-028"], "typography selector applies styles and tracks the caret"),
  { tag: specTags(["PAGE-028"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId, pageId } = await openFreshPage(driver, seed, session, "typo");
    try {
      await driver.pagesToolbarSettle(seed.workspaceSlug, projectId, pageId);
      expect(await driver.pagesToolbarTypographyItems()).toEqual([
        "Text",
        "Heading 1",
        "Heading 2",
        "Heading 3",
        "Heading 4",
        "Heading 5",
        "Heading 6",
      ]);

      await driver.pagesLiveTypeInBody("AAA");
      await driver.pagesToolbarPressKey("Enter");
      await driver.pagesToolbarTypeText("BBB");
      expect(await driver.pagesToolbarTypography()).toBe("Text");

      // Caret to the document start, then restyle the first block.
      await driver.pagesToolbarPressKey("ControlOrMeta+Home");
      await driver.pagesToolbarTypographyPick("Heading 2");
      await expect.poll(() => driver.pagesToolbarTypography(), { timeout: 15_000 }).toBe("Heading 2");
      await expect.poll(() => driver.pagesToolbarBodyHtml(), { timeout: 15_000 }).toContain("<h2");

      // Moving the caret into the plain block moves the selector back.
      await driver.pagesToolbarPressKey("ArrowDown");
      await expect.poll(() => driver.pagesToolbarTypography(), { timeout: 15_000 }).toBe("Text");
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-029"], "mark buttons toggle with tooltips and active highlights"),
  { tag: specTags(["PAGE-029"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId, pageId } = await openFreshPage(driver, seed, session, "marks");
    try {
      await driver.pagesToolbarSettle(seed.workspaceSlug, projectId, pageId);
      expect(await driver.pagesToolbarIconCount()).toBe(14);

      await driver.pagesLiveTypeInBody("mmm");
      const cases = [
        { index: 0, name: "Bold", hint: "Cmd + B", tag: "strong" },
        { index: 1, name: "Italic", hint: "Cmd + I", tag: "em" },
        { index: 2, name: "Underline", hint: "Cmd + U", tag: "u" },
        { index: 3, name: "Strikethrough", hint: "Cmd + Shift + S", tag: "s" },
      ];
      for (const mark of cases) {
        await test.step(`${mark.name} toggles`, async () => {
          expect(await driver.pagesToolbarTooltip(mark.index)).toEqual({ name: mark.name, hint: mark.hint });
          await driver.pagesToolbarSelectAll();
          expect(await driver.pagesToolbarActive(mark.index)).toBe(false);
          await expectToggle(driver, mark.index, `<${mark.tag}`, true);
          expect(await driver.pagesToolbarActive(mark.index)).toBe(true);
          await expectToggle(driver, mark.index, `<${mark.tag}`, false);
          expect(await driver.pagesToolbarActive(mark.index)).toBe(false);
        });
      }
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-030"], "alignment buttons realign with tooltips and active highlights"),
  { tag: specTags(["PAGE-030"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId, pageId } = await openFreshPage(driver, seed, session, "align");
    try {
      await driver.pagesToolbarSettle(seed.workspaceSlug, projectId, pageId);
      await driver.pagesLiveTypeInBody("aaa");
      const cases = [
        { index: 4, name: "Left align", hint: "Cmd + Shift + L", style: "left" },
        { index: 5, name: "Center align", hint: "Cmd + Shift + E", style: "center" },
        { index: 6, name: "Right align", hint: "Cmd + Shift + R", style: "right" },
      ];
      for (const alignment of cases) {
        await test.step(`${alignment.name} realigns`, async () => {
          expect(await driver.pagesToolbarTooltip(alignment.index)).toEqual({
            name: alignment.name,
            hint: alignment.hint,
          });
          await expectToggle(driver, alignment.index, new RegExp(`text-align:\\s*${alignment.style}`), true);
          expect(await driver.pagesToolbarActive(alignment.index)).toBe(true);
        });
      }
      // Alignment is exclusive: only the last pick stays highlighted.
      expect(await driver.pagesToolbarActive(4)).toBe(false);
      expect(await driver.pagesToolbarActive(5)).toBe(false);
      expect(await driver.pagesToolbarActive(6)).toBe(true);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-031"], "list buttons convert with tooltips and active highlights"),
  { tag: specTags(["PAGE-031"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId, pageId } = await openFreshPage(driver, seed, session, "lists");
    try {
      await driver.pagesToolbarSettle(seed.workspaceSlug, projectId, pageId);
      await driver.pagesLiveTypeInBody("items");
      const cases = [
        { index: 7, name: "Numbered list", hint: "Cmd + Shift + 7", marker: "<ol" },
        { index: 8, name: "Bulleted list", hint: "Cmd + Shift + 8", marker: "<ul" },
        { index: 9, name: "To-do list", hint: "Cmd + Shift + 9", marker: "taskList" },
      ];
      for (const list of cases) {
        await test.step(`${list.name} converts`, async () => {
          expect(await driver.pagesToolbarTooltip(list.index)).toEqual({ name: list.name, hint: list.hint });
          await expectToggle(driver, list.index, list.marker, true);
          expect(await driver.pagesToolbarActive(list.index)).toBe(true);
          await expectToggle(driver, list.index, list.marker, false);
          expect(await driver.pagesToolbarActive(list.index)).toBe(false);
        });
      }
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-032"], "quote and code controls restyle and persist through save and reload"),
  { tag: specTags(["PAGE-032"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId, pageId } = await openFreshPage(driver, seed, session, "quotecode");
    try {
      await driver.pagesToolbarSettle(seed.workspaceSlug, projectId, pageId);
      expect(await driver.pagesToolbarTooltip(10)).toEqual({ name: "Quote", hint: null });
      expect(await driver.pagesToolbarTooltip(11)).toEqual({ name: "Code", hint: null });

      await driver.pagesLiveTypeInBody("quoted");
      await expectToggle(driver, 10, "<blockquote", true);
      expect(await driver.pagesToolbarActive(10)).toBe(true);
      await driver.pagesToolbarSave();
      await pagesToolbarPollDescriptionHtml(seed.workspaceSlug, projectId, pageId, session, "<blockquote");
      await driver.pagesLiveOpenPageDetail(seed.workspaceSlug, projectId, pageId);
      await expect.poll(() => driver.pagesToolbarBodyHtml(), { timeout: 60_000 }).toContain("<blockquote");

      await driver.pagesToolbarSettle(seed.workspaceSlug, projectId, pageId);
      await expectToggle(driver, 11, "<code", true);
      expect(await driver.pagesToolbarActive(11)).toBe(true);
      await driver.pagesToolbarSave();
      await pagesToolbarPollDescriptionHtml(seed.workspaceSlug, projectId, pageId, session, "<code");
      await driver.pagesLiveOpenPageDetail(seed.workspaceSlug, projectId, pageId);
      await expect.poll(() => driver.pagesToolbarBodyHtml(), { timeout: 60_000 }).toContain("<code");
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-033"], "table insertion drops a table with keyboard navigation and persists"),
  { tag: specTags(["PAGE-033"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId, pageId } = await openFreshPage(driver, seed, session, "table");
    try {
      await driver.pagesToolbarSettle(seed.workspaceSlug, projectId, pageId);
      expect(await driver.pagesToolbarTooltip(12)).toEqual({ name: "Table", hint: null });

      await driver.pagesLiveTypeInBody("before");
      await driver.pagesToolbarClick(12);
      await expect.poll(() => driver.pagesToolbarTableDims(), { timeout: 15_000 }).toBe("3x3");
      const html = await driver.pagesToolbarBodyHtml();
      expect(html.indexOf("before")).toBeGreaterThan(-1);
      expect(html.indexOf("<table")).toBeGreaterThan(html.indexOf("before"));

      // Tab walks the caret across cells.
      const first = await driver.pagesToolbarCaretCell();
      expect(first).toMatch(/^r\d+c\d+$/);
      await driver.pagesToolbarPressKey("Tab");
      const second = await driver.pagesToolbarCaretCell();
      expect(second).toMatch(/^r\d+c\d+$/);
      expect(second).not.toBe(first);
      await driver.pagesToolbarPressKey("Tab");
      const third = await driver.pagesToolbarCaretCell();
      expect(third).toMatch(/^r\d+c\d+$/);
      expect(third).not.toBe(second);

      await driver.pagesToolbarSave();
      await pagesToolbarPollDescriptionHtml(seed.workspaceSlug, projectId, pageId, session, "<table");
      await driver.pagesLiveOpenPageDetail(seed.workspaceSlug, projectId, pageId);
      await expect.poll(() => driver.pagesToolbarTableDims(), { timeout: 60_000 }).toBe("3x3");
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-091"], "table row and column handles restructure the saved table"),
  { tag: specTags(["PAGE-091"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId, pageId } = await openFreshPage(driver, seed, session, "tablehandles");
    try {
      await driver.pagesToolbarSettle(seed.workspaceSlug, projectId, pageId);
      await driver.pagesLiveTypeInBody("before");
      await driver.pagesToolbarClick(12);
      await expect.poll(() => driver.pagesToolbarTableDims(), { timeout: 15_000 }).toBe("3x3");

      // Handles raise while editing; both menus hold their five entries
      // in order above the colour section.
      expect(await driver.pagesToolbarTableHandleVisible()).toBe(true);
      await expect
        .poll(() => driver.pagesToolbarTableMenuEntries("row"), { timeout: 15_000 })
        .toEqual(["Insert above", "Insert below", "Duplicate", "Clear contents", "Delete"]);
      await expect
        .poll(() => driver.pagesToolbarTableMenuEntries("column"), { timeout: 15_000 })
        .toEqual(["Insert left", "Insert right", "Duplicate", "Clear contents", "Delete"]);

      // Both drivers act on the first row's handle, so the mark must go
      // into the hovered row: the menu reads leave the caret wherever each
      // editor drops it, so re-anchor a collapsed caret on r0c0 first (a
      // key walk cannot be relied on while a line range is selected). Then
      // prove duplication copies content (the mark doubles), deleting the
      // first row drops the original (the mark singles), and clearing the
      // first row empties it.
      await driver.pagesToolbarTableClickCell(0, 0);
      expect(await driver.pagesToolbarCaretCell()).toBe("r0c0");
      await driver.pagesToolbarTypeText("dup-me");
      await driver.pagesToolbarTableMenuPick("row", "Duplicate");
      await expect.poll(() => driver.pagesToolbarTableDims(), { timeout: 15_000 }).toBe("4x3");
      expect((await driver.pagesToolbarBodyHtml()).split("dup-me").length).toBe(3);
      await driver.pagesToolbarTableMenuPick("row", "Delete");
      await expect.poll(() => driver.pagesToolbarTableDims(), { timeout: 15_000 }).toBe("3x3");
      expect((await driver.pagesToolbarBodyHtml()).split("dup-me").length).toBe(2);
      await driver.pagesToolbarTableMenuPick("row", "Clear contents");
      await expect.poll(() => driver.pagesToolbarBodyHtml(), { timeout: 15_000 }).not.toContain("dup-me");
      await expect.poll(() => driver.pagesToolbarTableDims(), { timeout: 15_000 }).toBe("3x3");

      // Columns mirror rows: the drivers act on the first column's
      // handle, so mark a cell in the hovered column (r1c0), prove the
      // duplicate carries the mark, then narrow back. Re-anchor on r0c0
      // first — Tabbing forward from an unknown caret could run off the
      // last cell and append a row instead of reaching r1c0.
      await driver.pagesToolbarTableClickCell(0, 0);
      expect(await driver.pagesToolbarCaretCell()).toBe("r0c0");
      for (let tab = 0; tab < 3; tab += 1) {
        await driver.pagesToolbarPressKey("Tab");
      }
      expect(await driver.pagesToolbarCaretCell()).toBe("r1c0");
      await driver.pagesToolbarTypeText("col-me");
      await driver.pagesToolbarTableMenuPick("column", "Duplicate");
      await expect.poll(() => driver.pagesToolbarTableDims(), { timeout: 15_000 }).toBe("3x4");
      expect((await driver.pagesToolbarBodyHtml()).split("col-me").length).toBe(3);
      await driver.pagesToolbarTableMenuPick("column", "Delete");
      await expect.poll(() => driver.pagesToolbarTableDims(), { timeout: 15_000 }).toBe("3x3");
      expect((await driver.pagesToolbarBodyHtml()).split("col-me").length).toBe(2);

      // Tab still walks the cells after restructuring. The menu
      // actions leave a range selection behind (in both editors), under
      // which Tab is inert — re-anchor a collapsed caret first, then
      // prove the walk both ways.
      await driver.pagesToolbarTableClickCell(0, 0);
      expect(await driver.pagesToolbarCaretCell()).toBe("r0c0");
      await driver.pagesToolbarPressKey("Tab");
      expect(await driver.pagesToolbarCaretCell()).toBe("r0c1");
      await driver.pagesToolbarPressKey("Shift+Tab");
      expect(await driver.pagesToolbarCaretCell()).toBe("r0c0");

      // A painted background covers the selected row and survives save
      // plus reload; clearing takes it back off.
      expect(await driver.pagesToolbarTableCellBackground(0, 0)).toBeNull();
      await driver.pagesToolbarTablePaint("row", 2);
      const painted = await driver.pagesToolbarTableCellBackground(0, 0);
      expect(painted).not.toBeNull();
      expect(await driver.pagesToolbarTableCellBackground(0, 2)).toBe(painted);
      expect(await driver.pagesToolbarTableCellBackground(1, 0)).toBeNull();
      await driver.pagesToolbarSave();
      await pagesToolbarPollDescriptionHtml(seed.workspaceSlug, projectId, pageId, session, "<table");
      await driver.pagesLiveOpenPageDetail(seed.workspaceSlug, projectId, pageId);
      await expect.poll(() => driver.pagesToolbarTableCellBackground(0, 0), { timeout: 60_000 }).toBe(painted);
      await expect.poll(() => driver.pagesToolbarTableDims(), { timeout: 60_000 }).toBe("3x3");
      await driver.pagesToolbarTablePaintClear("row");
      await expect.poll(() => driver.pagesToolbarTableCellBackground(0, 0), { timeout: 15_000 }).toBeNull();

      // Deleting every row leaves a paragraph, not a table.
      await driver.pagesToolbarTableMenuPick("row", "Delete");
      await driver.pagesToolbarTableMenuPick("row", "Delete");
      await driver.pagesToolbarTableMenuPick("row", "Delete");
      await expect.poll(() => driver.pagesToolbarTableDims(), { timeout: 15_000 }).toBeNull();
      expect(await driver.pagesToolbarBodyHtml()).toContain("<p");

      // Locked pages render no handles: the bar hides (proving the
      // locked view rendered) and no handle raises over the table.
      const lockedId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("tablehandles")} locked`);
      await test.step("locked pages render no table handles", async () => {
        await driver.pagesLiveOpenPageDetail(seed.workspaceSlug, projectId, lockedId);
        await driver.pagesToolbarSettle(seed.workspaceSlug, projectId, lockedId);
        await driver.pagesLiveTypeInBody("gated");
        await driver.pagesToolbarClick(12);
        await expect.poll(() => driver.pagesToolbarTableDims(), { timeout: 15_000 }).toBe("3x3");
        await driver.pagesToolbarSave();
        await pagesToolbarPollDescriptionHtml(seed.workspaceSlug, projectId, lockedId, session, "<table");
        await pagesLiveSetPageLock(seed.workspaceSlug, projectId, lockedId, true, session);
        await driver.pagesLiveOpenPageDetail(seed.workspaceSlug, projectId, lockedId);
        expect(await driver.pagesToolbarVisible()).toBe(false);
        expect(await driver.pagesToolbarTableHandleVisible()).toBe(false);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-034"], "toolbar image entry uploads through the file pipeline and renders inline"),
  { tag: specTags(["PAGE-034"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId, pageId } = await openFreshPage(driver, seed, session, "image");
    try {
      await driver.pagesToolbarSettle(seed.workspaceSlug, projectId, pageId);
      expect(await driver.pagesToolbarTooltip(13)).toEqual({ name: "Image", hint: null });

      await driver.pagesLiveTypeInBody("pic");
      await driver.pagesToolbarImageUpload(pngPath("image"));
      expect(await driver.pagesToolbarEditorHasImage()).toBe(true);
      const src = await driver.pagesToolbarImageSrc();
      expect(src).toContain("/api/assets/");

      // The file pipeline registered the bytes: the asset URL serves them.
      const asset = await pagesToolbarAssetBytes(src ?? "", session);
      expect(asset.status).toBe(200);
      expect(asset.byteLength).toBeGreaterThan(0);
      expect(asset.contentType).toContain("image/");

      // The image persists as an <image-component> node (asset uuid in
      // src, upload status) rather than a literal <img>.
      await driver.pagesToolbarSave();
      const saved = await pagesToolbarPollDescriptionHtml(
        seed.workspaceSlug,
        projectId,
        pageId,
        session,
        "<image-component"
      );
      expect(saved).toContain('status="uploaded"');
      await driver.pagesLiveOpenPageDetail(seed.workspaceSlug, projectId, pageId);
      await expect.poll(() => driver.pagesToolbarEditorHasImage(), { timeout: 60_000 }).toBe(true);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-035"], "formatting bar hides for read-only viewers and when the pref is off"),
  { tag: specTags(["PAGE-035"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId } = await scratchProject(seed, "barhide", session);
    const lockedId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("barhide")} locked`);
    const prefId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("barhide")} pref`);
    const onId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("barhide")} on`);
    try {
      await pagesLiveSetPageLock(seed.workspaceSlug, projectId, lockedId, true, session);
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("locked pages hide the bar and keep the floating pane toggle", async () => {
        await driver.pagesLiveOpenPageDetail(seed.workspaceSlug, projectId, lockedId);
        expect(await driver.pagesToolbarVisible()).toBe(false);
        expect(await driver.pagesToolbarFloatingToggle()).toBe(true);
      });

      await test.step("sticky-pref off hides the bar and keeps the floating toggle", async () => {
        await driver.pagesToolbarSeedStickyPref(false);
        await driver.pagesLiveOpenPageDetail(seed.workspaceSlug, projectId, prefId);
        expect(await driver.pagesToolbarVisible()).toBe(false);
        expect(await driver.pagesToolbarFloatingToggle()).toBe(true);
      });

      await test.step("sticky-pref on shows the bar (control)", async () => {
        await driver.pagesToolbarSeedStickyPref(true);
        await driver.pagesLiveOpenPageDetail(seed.workspaceSlug, projectId, onId);
        expect(await driver.pagesToolbarVisible()).toBe(true);
        expect(await driver.pagesToolbarFloatingToggle()).toBe(false);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-036"], "letter colors preview, apply, persist, and clear"),
  { tag: specTags(["PAGE-036"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId, pageId } = await openFreshPage(driver, seed, session, "colortext");
    try {
      await driver.pagesToolbarSettle(seed.workspaceSlug, projectId, pageId);
      expect(await driver.pagesToolbarColorPreview()).toEqual({ text: null, background: null });

      await driver.pagesToolbarColorOpen();
      expect(await driver.pagesToolbarColorCounts()).toEqual({
        text: 8,
        textClear: true,
        background: 8,
        backgroundClear: true,
      });
      await driver.pagesToolbarPressKey("Escape");

      await driver.pagesLiveTypeInBody("colors");
      await applyColor(driver, "text", 1, "data-text-color");
      expect((await driver.pagesToolbarColorPreview()).text).not.toBe(null);
      await driver.pagesToolbarSave();
      await pagesToolbarPollDescriptionHtml(seed.workspaceSlug, projectId, pageId, session, "data-text-color");
      await driver.pagesLiveOpenPageDetail(seed.workspaceSlug, projectId, pageId);
      await expect.poll(() => driver.pagesToolbarBodyHtml(), { timeout: 60_000 }).toContain("data-text-color");

      await driver.pagesToolbarSettle(seed.workspaceSlug, projectId, pageId);
      await clearColor(driver, "text", "data-text-color");
      expect(await driver.pagesLiveBodyText()).toContain("colors");
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-036"], "wash colors preview, apply, persist, and clear"),
  { tag: specTags(["PAGE-036"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { projectId, pageId } = await openFreshPage(driver, seed, session, "colorwash");
    try {
      await driver.pagesToolbarSettle(seed.workspaceSlug, projectId, pageId);
      await driver.pagesLiveTypeInBody("colors");
      await applyColor(driver, "background", 0, "data-background-color");
      expect((await driver.pagesToolbarColorPreview()).background).not.toBe(null);
      await driver.pagesToolbarSave();
      await pagesToolbarPollDescriptionHtml(seed.workspaceSlug, projectId, pageId, session, "data-background-color");
      await driver.pagesLiveOpenPageDetail(seed.workspaceSlug, projectId, pageId);
      await expect.poll(() => driver.pagesToolbarBodyHtml(), { timeout: 60_000 }).toContain("data-background-color");

      await driver.pagesToolbarSettle(seed.workspaceSlug, projectId, pageId);
      await clearColor(driver, "background", "data-background-color");
      expect(await driver.pagesLiveBodyText()).toContain("colors");
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
