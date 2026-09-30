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
  // Projects list + lifecycle (NEWFRONT-124, rows SHELL-024..045).
  // Selectors follow the old app's projects-list DOM as read from source
  // (card is a <Link href=".../projects/{id}/issues"> with an <h3> name and a
  // <p> short code; empty states render an <h3> heading; confirm dialogs are
  // ModalCore with a heading + named buttons + placeholder-only inputs).
  // User-visible targeting (getByRole/getByText/getByPlaceholder) throughout;
  // no data-testid is added to apps/web. The oracle driver is extended here,
  // never forked.
  // -------------------------------------------------------------------------

  /** A project card, located by the name heading inside a project link. */
  private cardByName(name: string): Locator {
    return this.page
      .locator('a[href*="/projects/"][href*="/issues"]')
      .filter({ has: this.page.getByRole("heading", { name, exact: true }) });
  }

  async openAuthenticatedAt(path: string, cookies: ParityBrowserCookie[]): Promise<void> {
    await this.page.context().addCookies(cookies);
    await this.page.goto(path);
    await this.page.waitForLoadState("domcontentloaded");
  }

  async currentUrlPath(): Promise<string> {
    return new URL(this.page.url()).pathname;
  }

  async hasText(text: string): Promise<boolean> {
    return this.isShown(this.page.getByText(text, { exact: false }));
  }

  async openProjectsList(workspaceSlug: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/projects`);
    await this.page.waitForLoadState("domcontentloaded");
  }

  async openArchivedProjects(workspaceSlug: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/projects/archives`);
    await this.page.waitForLoadState("domcontentloaded");
  }

  async visibleProjectCardNames(): Promise<string[]> {
    const headings = this.page.locator('a[href*="/projects/"][href*="/issues"]').getByRole("heading");
    const texts = await headings.allTextContents();
    return texts.map((t) => t.trim()).filter((t) => t.length > 0);
  }

  async awaitProjectCard(name: string): Promise<void> {
    await this.cardByName(name).first().waitFor({ timeout: 60_000 });
  }

  async gridColumnCount(): Promise<number> {
    // The card grid sets grid-template-columns; count the resolved tracks.
    const grid = this.page.locator('[class*="grid-cols-"]').filter({ has: this.page.locator('a[href*="/projects/"]') });
    const cols = await grid.first().evaluate((el) => {
      const tpl = getComputedStyle(el).gridTemplateColumns;
      return tpl.split(" ").filter((t) => t.trim().length > 0).length;
    });
    return cols;
  }

  async setViewportWidth(width: number): Promise<void> {
    await this.page.setViewportSize({ width, height: 1000 });
  }

  async isProjectsSkeletonVisible(): Promise<boolean> {
    const shimmer = this.page.locator(".animate-pulse");
    const names = await this.visibleProjectCardNames();
    return names.length === 0 && (await this.isShown(shimmer));
  }

  async emptyStateHeading(): Promise<string | null> {
    for (const text of ["No active projects", "No matching results.", "No projects archived"]) {
      const loc = this.page.getByRole("heading", { name: text, exact: false });
      if (await this.isShown(loc)) return (await loc.first().textContent())?.trim() ?? text;
    }
    return null;
  }

  private emptyStateCreateButton(): Locator {
    return this.page.getByRole("button", { name: "Start your first project", exact: true });
  }

  async isEmptyStateCreateVisible(): Promise<boolean> {
    return this.isShown(this.emptyStateCreateButton());
  }

  async isEmptyStateCreateEnabled(): Promise<boolean> {
    return this.emptyStateCreateButton().isEnabled();
  }

  async clickEmptyStateCreate(): Promise<void> {
    await this.emptyStateCreateButton().click();
  }

  private headerCreateButton(): Locator {
    // Header create button: label "Add Project" (>=sm) or "Project" (below sm).
    return this.page.getByRole("button", { name: /^(Add Project|Project)$/ });
  }

  async isHeaderCreateButtonVisible(): Promise<boolean> {
    return this.isShown(this.headerCreateButton());
  }

  async headerCreateButtonLabel(): Promise<string | null> {
    const btn = this.headerCreateButton();
    if (!(await this.isShown(btn))) return null;
    return (await btn.first().textContent())?.trim() ?? null;
  }

  async clickHeaderCreateButton(): Promise<void> {
    await this.headerCreateButton().first().click();
  }

  async breadcrumbLabels(): Promise<string[]> {
    // Breadcrumb items render inside the app header nav; read their text.
    const nav = this.page.getByRole("navigation").first();
    const texts = (await this.isShown(nav))
      ? await nav.getByRole("listitem").allTextContents()
      : await this.page.locator('[class*="breadcrumb" i] li, nav li').allTextContents();
    return texts.map((t) => t.trim()).filter((t) => t.length > 0);
  }

  async isMobileListHeaderVisible(): Promise<boolean> {
    // Mobile header order-by/filter bar lives in an md:hidden container.
    const orderBy = this.page.locator('[class*="md:hidden"]').getByRole("button", { name: /Filters/ });
    return this.isShown(orderBy);
  }

  async isDesktopFilterRowVisible(): Promise<boolean> {
    const row = this.page.locator('[class*="hidden"][class*="md:flex"]').getByRole("button", { name: "Filters" });
    return this.isShown(row);
  }

  async breadcrumbTerminalIsLink(): Promise<boolean> {
    const nav = this.page.getByRole("navigation").first();
    const items = (await this.isShown(nav)) ? nav.getByRole("listitem") : this.page.locator("nav li");
    const count = await items.count();
    if (count === 0) return false;
    const last = items.nth(count - 1);
    return (await last.getByRole("link").count()) > 0;
  }

  private sortTrigger(): Locator {
    // Order-by trigger shows the current option label among Manual/Name/...
    return this.page.getByRole("button", { name: /(Manual|Name|Created date|Number of members)/ }).first();
  }

  async openSortMenu(): Promise<void> {
    await this.sortTrigger().click();
  }

  async selectSortOption(label: string): Promise<void> {
    await this.page.getByRole("menuitem", { name: label, exact: true }).first().click();
  }

  async currentSortLabel(): Promise<string> {
    return (await this.sortTrigger().textContent())?.trim() ?? "";
  }

  async isSortDirectionDisabled(): Promise<boolean> {
    const asc = this.page.getByRole("menuitem", { name: "Ascending", exact: true }).first();
    return asc.isDisabled().catch(() => true);
  }

  async closeMenu(): Promise<void> {
    await this.page.keyboard.press("Escape");
  }

  private filterTrigger(): Locator {
    return this.page.getByRole("button", { name: "Filters" }).first();
  }

  async openFilterMenu(): Promise<void> {
    await this.filterTrigger().click();
  }

  async typeFilterSearch(text: string): Promise<void> {
    await this.page.getByPlaceholder("Search").last().fill(text);
  }

  async filterMenuHasOption(text: string): Promise<boolean> {
    return this.isShown(this.page.getByText(text, { exact: true }));
  }

  async selectFilterOption(label: string): Promise<void> {
    await this.page.getByText(label, { exact: true }).first().click();
  }

  async isFilterBadgeVisible(): Promise<boolean> {
    // Active-filter dot renders as a small accent span on the trigger.
    const dot = this.filterTrigger().locator('span[class*="bg-accent-primary"]');
    return this.isShown(dot);
  }

  private appliedFilterStrip(): Locator {
    // The applied-filters strip sits above the grid; scope chips to it.
    return this.page.locator('[class*="flex"][class*="flex-wrap"]').filter({ hasText: "Clear all" }).first();
  }

  async appliedFilterChipTexts(): Promise<string[]> {
    const strip = this.appliedFilterStrip();
    if (!(await this.isShown(strip))) return [];
    const texts = await strip.locator("span, div").allTextContents();
    return texts.map((t) => t.trim()).filter((t) => t.length > 0);
  }

  async removeAppliedFilterChip(text: string): Promise<void> {
    const chip = this.page.locator("*").filter({ hasText: text }).last();
    await chip.getByRole("button").last().click();
  }

  async clickClearAllFilters(): Promise<void> {
    await this.page.getByText("Clear all", { exact: true }).first().click();
  }

  async filterMatchCountText(): Promise<string | null> {
    const count = this.page.getByText(/^\d+\/\d+$/).first();
    if (!(await this.isShown(count))) return null;
    return (await count.textContent())?.trim() ?? null;
  }

  private listSearchInput(): Locator {
    return this.page.getByPlaceholder("Search").first();
  }

  async openListSearch(): Promise<void> {
    if (await this.isShown(this.listSearchInput())) return;
    await this.page
      .getByRole("button")
      .filter({ has: this.page.locator('svg[class*="search" i]') })
      .first()
      .click();
  }

  async typeListSearch(text: string): Promise<void> {
    await this.listSearchInput().fill(text);
  }

  async listSearchValue(): Promise<string> {
    return this.listSearchInput().inputValue();
  }

  async isListSearchExpanded(): Promise<boolean> {
    return this.isShown(this.listSearchInput());
  }

  async pressEscapeInListSearch(): Promise<void> {
    await this.listSearchInput().press("Escape");
  }

  async clickListSearchClear(): Promise<void> {
    await this.listSearchInput().locator("xpath=following-sibling::button").first().click();
  }

  async clickOutsideListSearch(): Promise<void> {
    await this.page.getByRole("main").click({ position: { x: 5, y: 5 } });
  }

  async cardShortCode(name: string): Promise<string | null> {
    const card = this.cardByName(name).first();
    if (!(await this.isShown(card))) return null;
    const text = await card.locator("p").first().textContent();
    return text?.trim() ?? null;
  }

  async cardHasPrivateMark(name: string): Promise<boolean> {
    const card = this.cardByName(name).first();
    return this.isShown(card.locator('svg[class*="lock" i]'));
  }

  async cardSubText(name: string): Promise<string | null> {
    const card = this.cardByName(name).first();
    const sub = card.locator("p.line-clamp-2, p[class*='line-clamp-2']").first();
    if (!(await this.isShown(sub))) return null;
    return (await sub.textContent())?.trim() ?? null;
  }

  private favoriteStar(name: string): Locator {
    const card = this.cardByName(name).first();
    return card.locator("button").filter({ has: this.page.locator('svg[class*="star" i]') });
  }

  async cardHasFavoriteStar(name: string): Promise<boolean> {
    return this.isShown(this.favoriteStar(name));
  }

  async clickFavoriteStar(name: string): Promise<void> {
    await this.favoriteStar(name).first().click();
  }

  async clickProjectCard(name: string): Promise<void> {
    await this.cardByName(name).first().click();
  }

  async openCardContextMenu(name: string): Promise<void> {
    await this.cardByName(name).first().click({ button: "right" });
  }

  async contextMenuItemLabels(): Promise<string[]> {
    const items = this.page.getByRole("menuitem");
    const texts = await items.allTextContents();
    return texts.map((t) => t.trim()).filter((t) => t.length > 0);
  }

  async clickContextMenuItem(label: string): Promise<void> {
    await this.page.getByRole("menuitem", { name: label, exact: true }).first().click();
  }

  async cardFooterLabels(name: string): Promise<string[]> {
    const card = this.cardByName(name).first();
    const texts = await card.getByRole("button").allTextContents();
    const links = await card.getByRole("link").allTextContents();
    return [...texts, ...links].map((t) => t.trim()).filter((t) => t.length > 0);
  }

  async clickCardJoin(name: string): Promise<void> {
    await this.cardByName(name).first().getByRole("button", { name: "Join", exact: true }).click();
  }

  async isJoinDialogVisible(): Promise<boolean> {
    return this.isShown(this.page.getByRole("heading", { name: "Join Project?", exact: true }));
  }

  async joinDialogHeading(): Promise<string | null> {
    const h = this.page.getByRole("heading", { name: "Join Project?", exact: true });
    if (!(await this.isShown(h))) return null;
    return (await h.first().textContent())?.trim() ?? null;
  }

  async confirmJoin(): Promise<void> {
    await this.page.getByRole("button", { name: "Join Project", exact: true }).click();
  }

  /**
   * The leave and archive actions are not on the projects-list card; the old
   * app surfaces them from the sidebar project item's action menu. Open that
   * menu for the named project and click the given action.
   */
  private async openSidebarProjectAction(projectName: string, action: RegExp): Promise<void> {
    const item = this.page.getByRole("link", { name: projectName, exact: false }).first();
    await item.hover();
    await item.locator("xpath=ancestor-or-self::*[1]").getByRole("button").last().click();
    await this.page.getByRole("menuitem", { name: action }).first().click();
  }

  async openLeaveProjectDialog(projectName: string): Promise<void> {
    await this.openSidebarProjectAction(projectName, /Leave/i);
  }

  async fillLeaveProjectName(text: string): Promise<void> {
    await this.page.getByPlaceholder("Enter project name").fill(text);
  }

  async fillLeaveConfirmPhrase(text: string): Promise<void> {
    await this.page.getByPlaceholder("Enter 'leave project'").fill(text);
  }

  async submitLeave(): Promise<void> {
    await this.page.getByRole("button", { name: "Leave Project", exact: true }).click();
  }

  async leaveErrorText(): Promise<string | null> {
    for (const text of [
      "Please enter the project name as shown in the description.",
      "Please confirm leaving the project by typing the 'Leave Project'.",
    ]) {
      const loc = this.page.getByText(text, { exact: false });
      if (await this.isShown(loc)) return (await loc.first().textContent())?.trim() ?? text;
    }
    return null;
  }

  async isLeaveDialogVisible(): Promise<boolean> {
    return this.isShown(this.page.getByRole("heading", { name: "Leave Project", exact: true }));
  }

  async openArchiveProjectDialog(projectName: string): Promise<void> {
    await this.openSidebarProjectAction(projectName, /Archive/i);
  }

  async archiveDialogBodyText(): Promise<string | null> {
    const body = this.page.getByText(/will be archived|Restoring a project/i).first();
    if (!(await this.isShown(body))) return null;
    return (await body.textContent())?.trim() ?? null;
  }

  async confirmArchive(): Promise<void> {
    await this.page.getByRole("button", { name: "Archive", exact: true }).click();
  }

  private archivedCard(name: string): Locator {
    return this.cardByName(name).first();
  }

  async clickCardRestore(name: string): Promise<void> {
    await this.archivedCard(name)
      .getByRole("button", { name: /Restore/ })
      .first()
      .click();
  }

  async confirmRestore(): Promise<void> {
    await this.page.getByRole("button", { name: "Restore", exact: true }).click();
  }

  async archivedCardHasAdminActions(name: string): Promise<boolean> {
    const card = this.archivedCard(name);
    return this.isShown(card.getByRole("button", { name: /Restore/ }));
  }

  async cardShowsArchivedMarker(name: string): Promise<boolean> {
    return this.isShown(this.archivedCard(name).getByText("Archived", { exact: false }));
  }

  async openDeleteProjectDialog(name: string): Promise<void> {
    await this.openCardContextMenu(name);
    await this.clickContextMenuItem("Delete");
  }

  async fillDeleteProjectName(text: string): Promise<void> {
    await this.page.getByPlaceholder("Project name").fill(text);
  }

  async fillDeleteConfirmPhrase(text: string): Promise<void> {
    await this.page.getByPlaceholder("Enter 'delete my project'").fill(text);
  }

  async isDeleteSubmitDisabled(): Promise<boolean> {
    return this.page.getByRole("button", { name: "Delete project", exact: true }).isDisabled();
  }

  async submitDelete(): Promise<void> {
    await this.page.getByRole("button", { name: "Delete project", exact: true }).click();
  }

  async isCreateProjectDialogVisible(): Promise<boolean> {
    return this.isShown(this.page.getByPlaceholder("Project name"));
  }

  async fillCreateProjectName(text: string): Promise<void> {
    await this.page.getByPlaceholder("Project name").fill(text);
  }

  async createProjectShortCodeValue(): Promise<string> {
    return this.page.getByPlaceholder("Project ID").inputValue();
  }

  async fillCreateProjectShortCode(text: string): Promise<void> {
    await this.page.getByPlaceholder("Project ID").fill(text);
  }

  async submitCreateProject(): Promise<void> {
    await this.page.getByRole("button", { name: "Create project", exact: true }).click();
  }

  async createProjectErrorText(): Promise<string | null> {
    for (const text of [
      "The project name is already taken.",
      "The project identifier is already taken.",
      "Cover image upload skipped — using a default cover.",
    ]) {
      const loc = this.page.getByText(text, { exact: false });
      if (await this.isShown(loc)) return (await loc.first().textContent())?.trim() ?? text;
    }
    return null;
  }
}
