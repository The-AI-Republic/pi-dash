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

  // -------------------------------------------------------------------------
  // Command palette / Power-K, search, help, browse, repo-star
  // (NEWFRONT-127, rows SHELL-080, 082, 083, 084, 085, 087, 089, 094, 103, 106).
  // The old app builds the palette on `cmdk` inside a Headless-UI dialog and
  // carries NO data-testid, so selectors target cmdk's own DOM attributes
  // ([cmdk-root]/[cmdk-input]/[cmdk-item]/[cmdk-group-heading]), placeholder
  // text ("Type a command or search"), aria-selected, and visible labels —
  // all user-visible. Derived from a source read of core/components/power-k;
  // the oracle driver is extended here, never forked.
  // -------------------------------------------------------------------------

  /** True when a locator resolves to at least one visible element. */
  private async isShown(locator: Locator): Promise<boolean> {
    return (await locator.count()) > 0 && (await locator.first().isVisible());
  }

  /**
   * The centered MODAL palette, scoped to its Headless-UI dialog so its cmdk
   * nodes never collide with the top-bar search box (SHELL-081), which embeds
   * its own cmdk surface inline (not in a dialog).
   */
  private paletteModal(): Locator {
    return this.page
      .getByRole("dialog")
      .filter({ has: this.page.locator("[cmdk-root]") })
      .first();
  }

  /** The cmdk command input of the open modal palette (root or a sub-page). */
  private paletteInput(): Locator {
    return this.paletteModal().locator("[cmdk-input]").first();
  }

  private paletteRoot(): Locator {
    return this.paletteModal().locator("[cmdk-root]").first();
  }

  private paletteItems(): Locator {
    return this.paletteModal().locator("[cmdk-item]");
  }

  async currentUrlPath(): Promise<string> {
    return new URL(this.page.url()).pathname;
  }

  async goToPath(path: string): Promise<void> {
    await this.page.goto(path);
    await this.page.waitForLoadState("domcontentloaded");
  }

  async hasVisibleText(text: string): Promise<boolean> {
    return this.isShown(this.page.getByText(text, { exact: false }));
  }

  // --- palette open / close / reset (SHELL-080, SHELL-082) ---

  async pressPaletteOpenChord(): Promise<void> {
    // The old app's handler catches Ctrl/Cmd+K on document before its typing
    // guard, so this opens the palette from anywhere, including inside inputs.
    await this.page.keyboard.press("ControlOrMeta+k");
  }

  async isCommandPaletteOpen(): Promise<boolean> {
    return this.isShown(this.paletteInput());
  }

  async commandPalettePlaceholder(): Promise<string | null> {
    const input = this.paletteInput();
    if (!(await this.isShown(input))) return null;
    return input.getAttribute("placeholder");
  }

  async focusAndTypeTopBarSearch(text: string): Promise<void> {
    // The ce top navigation always mounts an expandable search input
    // (placeholder "Search commands..."). Focus it and type to prove the
    // open chord still fires while a text field holds focus.
    const search = this.page.getByPlaceholder("Search commands...").first();
    await search.click();
    await search.pressSequentially(text);
  }

  async closeCommandPaletteViaBackdrop(): Promise<void> {
    // Headless-UI renders the backdrop as a fixed full-screen layer behind the
    // panel; a top-left click lands on it, not on the centered max-w-2xl panel.
    await this.page.mouse.click(5, 5);
  }

  // --- palette query + keyboard flow (SHELL-083, SHELL-085) ---

  async typeInCommandPalette(text: string): Promise<void> {
    await this.paletteInput().pressSequentially(text);
  }

  async commandPaletteQueryValue(): Promise<string> {
    return this.paletteInput().inputValue();
  }

  async pressInCommandPalette(key: string): Promise<void> {
    await this.paletteInput().press(key);
  }

  async paletteGroupHeadings(): Promise<string[]> {
    const texts = await this.paletteModal().locator("[cmdk-group-heading]").allTextContents();
    return texts.map((t) => t.trim()).filter((t) => t.length > 0);
  }

  async paletteCommandTitles(): Promise<string[]> {
    // Each cmdk item renders an icon, its title, and optional shortcut badges;
    // the trimmed text content is dominated by the title. Callers match by
    // substring (paletteHasCommand) rather than exact equality.
    const texts = await this.paletteItems().allTextContents();
    return texts.map((t) => t.replace(/\s+/g, " ").trim()).filter((t) => t.length > 0);
  }

  async paletteHasCommand(title: string): Promise<boolean> {
    return this.isShown(this.paletteItems().filter({ hasText: title }));
  }

  async activatePaletteCommand(title: string): Promise<void> {
    await this.paletteItems().filter({ hasText: title }).first().click();
  }

  async paletteSelectedItemText(): Promise<string | null> {
    const selected = this.paletteModal().locator('[cmdk-item][aria-selected="true"]').first();
    if (!(await this.isShown(selected))) return null;
    return (await selected.textContent())?.replace(/\s+/g, " ").trim() ?? null;
  }

  // --- server search (SHELL-084) ---

  private searchResultsHeading(): Locator {
    return this.paletteModal()
      .getByText(/Search results for/i)
      .first();
  }

  async paletteSearchResultsHeading(): Promise<string | null> {
    const heading = this.searchResultsHeading();
    if (!(await this.isShown(heading))) return null;
    return (await heading.textContent())?.trim() ?? null;
  }

  async isPaletteSearchHeadingPulsing(): Promise<boolean> {
    const heading = this.searchResultsHeading();
    if ((await heading.count()) === 0) return false;
    const cls = (await heading.getAttribute("class")) ?? "";
    return cls.includes("animate-pulse");
  }

  private workspaceLevelToggle(): Locator {
    // Footer scope control labelled "Workspace level"; the ToggleSwitch is the
    // adjacent switch/button.
    return this.paletteRoot()
      .locator("*")
      .filter({ hasText: /Workspace level/i })
      .getByRole("switch")
      .first();
  }

  async paletteHasWorkspaceLevelToggle(): Promise<boolean> {
    return this.isShown(
      this.paletteModal()
        .getByText(/Workspace level/i)
        .first()
    );
  }

  async isWorkspaceLevelToggleEnabled(): Promise<boolean> {
    const toggle = this.workspaceLevelToggle();
    if ((await toggle.count()) === 0) return false;
    return toggle.isEnabled();
  }

  async toggleWorkspaceLevel(): Promise<void> {
    await this.workspaceLevelToggle().click();
  }

  private searchRequests: URL[] = [];
  private searchTrackingOn = false;

  private ensureSearchTracking(): void {
    if (this.searchTrackingOn) return;
    this.searchTrackingOn = true;
    this.page.on("request", (req) => {
      const url = req.url();
      if (/\/api\/workspaces\/[^/]+\/search\/?(\?|$)/.test(url)) {
        this.searchRequests.push(new URL(url));
      }
    });
  }

  async countSearchRequests(action: () => Promise<void>): Promise<number> {
    this.ensureSearchTracking();
    const before = this.searchRequests.length;
    await action();
    return this.searchRequests.length - before;
  }

  async lastSearchRequestParams(): Promise<Record<string, string> | null> {
    const last = this.searchRequests.at(-1);
    if (!last) return null;
    return Object.fromEntries(last.searchParams.entries());
  }

  // --- shortcuts reference dialog (SHELL-094) ---

  async isShortcutsDialogOpen(): Promise<boolean> {
    return this.isShown(this.page.getByText("Keyboard shortcuts", { exact: false }));
  }

  async pressShortcutsDialogChord(): Promise<void> {
    await this.page.keyboard.press("ControlOrMeta+/");
  }

  async typeShortcutsFilter(text: string): Promise<void> {
    await this.page.getByPlaceholder("Search for shortcuts").fill(text);
  }

  async shortcutsDialogCommandTitles(): Promise<string[]> {
    const dialog = this.page
      .getByRole("dialog")
      .filter({ has: this.page.getByPlaceholder("Search for shortcuts") })
      .first();
    const root = (await dialog.count()) > 0 ? dialog : this.page.getByRole("dialog").first();
    const texts = await root.locator("h5 ~ * , li, [class*='flex']").allTextContents();
    return texts.map((t) => t.replace(/\s+/g, " ").trim()).filter((t) => t.length > 0);
  }

  // --- repo-star action (SHELL-103) ---

  async repoStarLinkAttributes(): Promise<{ href: string; target: string; rel: string } | null> {
    const link = this.page.getByRole("link", { name: /Star us on GitHub/i }).first();
    if ((await link.count()) === 0) return null;
    return {
      href: (await link.getAttribute("href")) ?? "",
      target: (await link.getAttribute("target")) ?? "",
      rel: (await link.getAttribute("rel")) ?? "",
    };
  }

  async repoStarIconSrc(): Promise<string | null> {
    const img = this.page
      .getByRole("link", { name: /Star us on GitHub/i })
      .first()
      .locator("img")
      .first();
    if ((await img.count()) === 0) return null;
    return img.getAttribute("src");
  }

  // --- browse route (SHELL-106, negative row) ---

  async openBrowseWorkItem(workspaceSlug: string, identifier: string): Promise<void> {
    await this.goToPath(`/${workspaceSlug}/browse/${identifier}`);
  }

  async browseShowsWorkItemDetail(): Promise<boolean> {
    // The browse route renders the same single-work-item detail root as the
    // in-project route: a main region with the work-item title and the detail
    // affordances (no card grid). Presence of the issue-detail region is the
    // positive signal.
    const main = this.page.getByRole("main");
    const hasDetail = await this.isShown(main);
    const hasCards = await this.isShown(this.page.locator('a[href*="/projects/"][href*="/issues"]'));
    return hasDetail && !hasCards;
  }

  async browseShowsWorkspaceWideList(): Promise<boolean> {
    // A cross-project browser would render many project/work-item cards or a
    // list grid; the negative row asserts none exists.
    const grid = this.page.locator('[class*="grid-cols-"]').filter({
      has: this.page.locator('a[href*="/projects/"]'),
    });
    return this.isShown(grid);
  }
}
