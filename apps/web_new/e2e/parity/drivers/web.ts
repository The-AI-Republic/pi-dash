// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle driver (NEWFRONT-19): implements the parity driver interface
// against apps/web. Selectors follow the sign-in card behavior observed on
// the running old app: the entry route renders the email step, a submit
// moves to the password step, and the password submit posts the native
// form, landing in the workspace. Later oracle issues extend this driver
// (never fork it) as new areas need new actions.
import type { Locator, Page } from "@playwright/test";
import type { ParityDriver, ParityTarget } from "./parity-driver";

export class WebDriver implements ParityDriver {
  readonly target: ParityTarget = "web";
  readonly page: Page;

  constructor(page: Page) {
    this.page = page;
  }

  async openEntry(): Promise<void> {
    await this.page.goto("/");
    await this.page.getByPlaceholder("name@company.com").first().waitFor();
  }

  private submitOf(form: Locator): Locator {
    return form.locator('button[type="submit"]');
  }

  async signInWithPassword(email: string, password: string): Promise<void> {
    const page = this.page;
    const emailField = page.getByPlaceholder("name@company.com").first();
    await emailField.fill(email);
    const emailForm = page.locator("form", { has: emailField });
    await this.submitOf(emailForm).click();
    const passwordField = page.getByPlaceholder("Enter password");
    await passwordField.waitFor();
    await passwordField.fill(password);
    const passwordForm = page.locator("form", { has: passwordField });
    // The old app posts the native form, so this ends in a full page load.
    await Promise.all([page.waitForURL(/\/[^/]+\//), this.submitOf(passwordForm).click()]);
  }

  async openProjectIssues(workspaceSlug: string, projectId: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/projects/${projectId}/issues`);
    await this.page.waitForLoadState("domcontentloaded");
  }

  /**
   * User-visible read of the issues list. Observed on the running old app:
   * each issue row renders its title as paragraph text inside a main
   * landmark, and every seeded title is unique on the page — so no
   * app-side hook is needed. Returns every non-empty paragraph text found
   * (this includes surrounding chrome such as nav labels); callers match
   * the names they care about out of it. Scenarios poll this until the
   * list populates instead of waiting on a fixed selector.
   */
  async visibleIssueNames(): Promise<string[]> {
    const texts = await this.page.getByRole("main").locator("p").allTextContents();
    return texts.map((t) => t.trim()).filter((t) => t.length > 0);
  }

  // --- Mention flows (NEWFRONT-115, CMT-019/020/021). Appended after the
  // --- NEWFRONT-19 methods, which are untouched per the shared contract.
  //
  // Observed on the running old app (issue detail, activity section):
  // the comment composer is the last rich-text editable on the page;
  // typing "@" opens a suggestion popup whose items are buttons with ids
  // like "mention-item-0-0" grouped under section headers ("Users");
  // Enter picks the highlighted suggestion; the "Comment" button posts
  // (a success toast confirms); saved comments render each mention as a
  // link to "/{workspaceSlug}/profile/{memberId}" showing "@Display Name".

  /**
   * Sign in with the shared flow and verify the session actually landed.
   * The shared sign-in posts the native form, and the backend rejects
   * valid credentials intermittently while sibling parity runs hammer the
   * shared scratch stack, so this retries the whole flow for up to 90
   * seconds: leaving the entry route proves the login POST went through,
   * and the sidebar printing the account email proves whose session it
   * is. Calls the shared methods; never edits them. Bounded at 60 seconds so
   * a scenario still fits the suite's per-test budget alongside its poll
   * waits; a window needing longer is broken for every scenario anyway.
   */
  /**
   * One sign-in attempt bounded wall-clock: the shared methods carry no
   * explicit timeouts of their own, so a stalled oracle would otherwise
   * hang an attempt until the suite's test timeout with no retry.
   */
  private async mentionsAttempt<T>(work: () => Promise<T>, ms: number, label: string): Promise<T> {
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      return await Promise.race([
        work(),
        new Promise<never>((_, reject) => {
          timer = setTimeout(() => reject(new Error(`[parity] ${label} timed out after ${ms}ms`)), ms);
        }),
      ]);
    } finally {
      if (timer !== undefined) clearTimeout(timer);
    }
  }

  async mentionsEnsureSignedIn(email: string, password: string): Promise<void> {
    const deadline = Date.now() + 60_000;
    let lastError = "no attempt finished";
    let attempts = 0;
    while (Date.now() < deadline && attempts < 3) {
      attempts += 1;
      try {
        await this.mentionsAttempt(
          async () => {
            await this.openEntry();
            await this.signInWithPassword(email, password);
            await this.page.waitForURL((url) => url.pathname !== "/", { timeout: 15_000 });
            await this.page.getByText(email).first().waitFor({ timeout: 15_000 });
          },
          45_000,
          `sign-in attempt ${attempts}`
        );
        return;
      } catch (error) {
        lastError = String(error).split("\n")[0];
      }
    }
    throw new Error(`[parity] sign-in as ${email} did not land (attempts: ${attempts}): ${lastError}`);
  }

  async mentionsOpenIssueDetail(workspaceSlug: string, projectId: string, issueId: string): Promise<void> {
    // Explicit navigation timeout: without one a stalled oracle hangs the
    // step until the suite's test timeout instead of failing diagnosably.
    await this.page.goto(`/${workspaceSlug}/projects/${projectId}/issues/${issueId}`, { timeout: 60_000 });
    await this.page.waitForLoadState("domcontentloaded");
    // The composer submit existing proves the detail plus its activity
    // section loaded authenticated; the app redirects this URL to the
    // canonical browse path, so no URL assertion here.
    try {
      await this.page.getByRole("button", { name: "Comment", exact: true }).first().waitFor({ timeout: 60_000 });
    } catch (error) {
      // Name the intake race distinctly: a sibling run can triage the
      // issue between selection and open, landing on the intake view
      // which has no composer. Anything else is slowness or auth.
      if (this.page.url().includes("/intake/")) {
        throw new Error(`[parity] issue ${issueId} landed on the intake view; it was triaged after selection.`);
      }
      throw error;
    }
  }

  private mentionsComposer(): Locator {
    return this.page.locator('[contenteditable="true"]').last();
  }

  private mentionsSuggestionButtons(): Locator {
    return this.page.locator('button[id^="mention-item-"]');
  }

  async mentionsSuggestionsFor(query: string): Promise<string[]> {
    const composer = this.mentionsComposer();
    // A suggestion popup left open by a previous query covers the composer
    // and would intercept the click; dismiss it first.
    await this.page.keyboard.press("Escape");
    await composer.scrollIntoViewIfNeeded();
    await composer.click();
    // Start from an empty composer so repeated calls re-query instead of
    // appending to the previous trigger text.
    await this.page.keyboard.press("ControlOrMeta+a");
    await this.page.keyboard.press("Backspace");
    await composer.pressSequentially(`@${query}`, { delay: 90 });
    const items = this.mentionsSuggestionButtons();
    await items.first().waitFor({ timeout: 30_000 });
    // The name paragraph excludes the avatar initial, which allTextContents
    // of the whole button would prepend.
    const names = await items.locator("p").allTextContents();
    return names.map((t) => t.trim()).filter((t) => t.length > 0);
  }

  async mentionsSuggestionsHaveAvatars(): Promise<boolean> {
    const items = this.mentionsSuggestionButtons();
    const count = await items.count();
    if (count === 0) return false;
    for (const item of await items.all()) {
      const avatar = item.locator("span").first();
      if ((await avatar.count()) === 0) return false;
      if (!((await avatar.evaluate((el) => (el.textContent ?? "").trim())).length > 0)) return false;
    }
    return true;
  }

  async mentionsSuggestionSections(): Promise<string[]> {
    const headers = this.page.locator(".react-renderer h6");
    await headers.first().waitFor({ timeout: 30_000 });
    const texts = await headers.allTextContents();
    return texts.map((t) => t.trim()).filter((t) => t.length > 0);
  }

  async mentionsPostComment(displayName: string, bodyText: string): Promise<void> {
    const composer = this.mentionsComposer();
    await this.page.keyboard.press("Escape");
    await composer.scrollIntoViewIfNeeded();
    await composer.click();
    await composer.pressSequentially(`@${displayName}`, { delay: 90 });
    await this.mentionsSuggestionButtons().first().waitFor({ timeout: 30_000 });
    await this.page.keyboard.press("Enter");
    await composer.pressSequentially(` ${bodyText}`, { delay: 40 });
    await this.page.getByRole("button", { name: "Comment", exact: true }).first().click();
    // The posted body text appearing in the feed proves the save landed;
    // bodyText is unique per scenario run, so this cannot match a stale row.
    await this.page.getByText(bodyText).first().waitFor({ timeout: 60_000 });
  }

  async mentionsVisibleReferences(): Promise<{ text: string; href: string | null }[]> {
    const links = this.page.locator('a[href*="/profile/"]');
    const count = await links.count();
    const refs: { text: string; href: string | null }[] = [];
    for (let i = 0; i < count; i++) {
      const link = links.nth(i);
      const text = (await link.allTextContents()).join("").trim();
      // Mention chips render "@Display Name"; other profile links (author
      // rows, assignees) do not carry the "@" marker.
      if (!text.startsWith("@")) continue;
      refs.push({ text, href: await link.getAttribute("href") });
    }
    return refs;
  }

  /**
   * Remove the mention from the comment showing `oldBodyText` through the
   * card overflow menu. Observed on the running old app: each comment card
   * header carries an ellipsis trigger opening a small popover with an
   * edit option; the inline edit form saves on Enter.
   */
  async mentionsEditRemovingMention(oldBodyText: string, plainText: string): Promise<void> {
    const page = this.page;
    const body = page.locator(`text=${oldBodyText}`).first();
    await body.waitFor({ timeout: 60_000 });
    await body.scrollIntoViewIfNeeded();
    // The header controls render on card hover.
    await body.hover();
    const trigger = await body.evaluateHandle((el) => {
      const origin = el.getBoundingClientRect();
      // Climb to the card root: the nearest ancestor that also renders the
      // card header ("commented … ago").
      let card: HTMLElement | null = el instanceof HTMLElement ? el : el.parentElement;
      while (card !== null && card !== document.body) {
        if ((card.innerText ?? "").includes("commented")) break;
        card = card.parentElement;
      }
      const scope = card ?? document.body;
      const originBox = origin;
      const scoreOf = (candidate: Element): number | null => {
        const box = candidate.getBoundingClientRect();
        if (box.width === 0 || box.height === 0) return null;
        const dy = originBox.top - box.top;
        if (dy < -10 || dy > 260) return null;
        return Math.abs(box.left + box.width / 2 - (originBox.left + originBox.width / 2)) + dy;
      };
      // The overflow trigger is an icon-only control: prefer a literal
      // ellipsis, else the rightmost header button. Proximity alone picks
      // the reaction button (same header, no text either), which opens the
      // emoji picker instead of the card menu.
      let best: Element | null = null;
      let bestScore = 1e12;
      for (const candidate of scope.querySelectorAll("button, div")) {
        const text = (candidate.textContent ?? "").trim();
        if (text !== "…" && text !== "...") continue;
        const score = scoreOf(candidate);
        if (score !== null && score < bestScore) {
          bestScore = score;
          best = candidate;
        }
      }
      if (best === null) {
        let bestX = -1e12;
        for (const candidate of scope.querySelectorAll("button")) {
          const text = (candidate.textContent ?? "").trim();
          if (text !== "") continue;
          const score = scoreOf(candidate);
          if (score === null) continue;
          const x = candidate.getBoundingClientRect().left;
          if (x > bestX) {
            bestX = x;
            best = candidate;
          }
        }
      }
      return best;
    });
    const triggerElement = trigger.asElement();
    if (triggerElement === null) throw new Error("[parity] comment overflow trigger not found.");
    await triggerElement.click({ force: true });
    // The popover option edits inline. Prefer a real button (the static
    // "Last edited …" header text must never match); fall back to anchored
    // visible text for non-button menu rows.
    const editButton = page.getByRole("button", { name: /^edit/i });
    const editOpened = await editButton
      .first()
      .waitFor({ timeout: 15_000 })
      .then(() => true)
      .catch(() => false);
    if (editOpened) {
      await editButton.first().click();
    } else {
      const editText = page.getByText(/^edit/i);
      await editText.first().waitFor({ timeout: 15_000 });
      await editText.first().click();
    }
    // The card swaps to an inline rich-text form pre-filled with the old
    // body; find the editable holding it (the page composer below the
    // feed is empty, so the text uniquely identifies the inline form).
    const editorHandle = await page.waitForFunction(
      (text) => {
        const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
        let node: Text | null = null;
        while ((node = walker.nextNode() as Text | null) !== null) {
          if ((node.textContent ?? "").includes(text)) break;
        }
        if (node === null || !(node.parentElement instanceof HTMLElement)) return null;
        const editable = node.parentElement.closest('[contenteditable="true"]');
        return editable instanceof HTMLElement ? editable : null;
      },
      oldBodyText,
      { timeout: 30_000 }
    );
    const editorElement = editorHandle.asElement();
    if (editorElement === null) throw new Error("[parity] inline comment editor not found.");
    await editorElement.click();
    await page.keyboard.press("ControlOrMeta+a");
    await page.keyboard.press("Backspace");
    await editorElement.type(plainText, { delay: 40 });
    await page.keyboard.press("Enter");
    // The edited plain text appearing while the old body disappears proves
    // the save landed.
    await page.getByText(plainText).first().waitFor({ timeout: 60_000 });
  }
}
