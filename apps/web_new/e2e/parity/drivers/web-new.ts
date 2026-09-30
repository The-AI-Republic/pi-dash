// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// New-app driver skeleton (NEWFRONT-19). Implements the same interface as
// the oracle driver so scenarios compile against either target, but every
// action throws until the matching area lands in apps/web_new. Area issues
// fill these in method by method; the oracle driver stays untouched.
import type { Page } from "@playwright/test";
import type { ParityDriver, ParityTarget } from "./parity-driver";

function todo(target: string): never {
  throw new Error(`[parity] drivers/web_new has no ${target} yet; the area that owns it has not landed.`);
}

export class WebNewDriver implements ParityDriver {
  readonly target: ParityTarget = "web_new";
  readonly page: Page;

  constructor(page: Page) {
    this.page = page;
  }

  async openEntry(): Promise<void> {
    return todo("openEntry");
  }

  async signInWithPassword(_email: string, _password: string): Promise<void> {
    return todo("signInWithPassword");
  }

  async openProjectIssues(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("openProjectIssues");
  }

  async visibleIssueNames(): Promise<string[]> {
    return todo("visibleIssueNames");
  }

  // Command palette / search / help / browse / repo-star (NEWFRONT-127).
  // Skeleton stubs: throw until the apps/web_new palette area lands.

  async currentUrlPath(): Promise<string> {
    return todo("currentUrlPath");
  }

  async goToPath(_path: string): Promise<void> {
    return todo("goToPath");
  }

  async hasVisibleText(_text: string): Promise<boolean> {
    return todo("hasVisibleText");
  }

  async pressPaletteOpenChord(): Promise<void> {
    return todo("pressPaletteOpenChord");
  }

  async isCommandPaletteOpen(): Promise<boolean> {
    return todo("isCommandPaletteOpen");
  }

  async commandPalettePlaceholder(): Promise<string | null> {
    return todo("commandPalettePlaceholder");
  }

  async focusAndTypeTopBarSearch(_text: string): Promise<void> {
    return todo("focusAndTypeTopBarSearch");
  }

  async closeCommandPaletteViaBackdrop(): Promise<void> {
    return todo("closeCommandPaletteViaBackdrop");
  }

  async typeInCommandPalette(_text: string): Promise<void> {
    return todo("typeInCommandPalette");
  }

  async commandPaletteQueryValue(): Promise<string> {
    return todo("commandPaletteQueryValue");
  }

  async pressInCommandPalette(_key: string): Promise<void> {
    return todo("pressInCommandPalette");
  }

  async paletteGroupHeadings(): Promise<string[]> {
    return todo("paletteGroupHeadings");
  }

  async paletteCommandTitles(): Promise<string[]> {
    return todo("paletteCommandTitles");
  }

  async paletteHasCommand(_title: string): Promise<boolean> {
    return todo("paletteHasCommand");
  }

  async activatePaletteCommand(_title: string): Promise<void> {
    return todo("activatePaletteCommand");
  }

  async paletteSelectedItemText(): Promise<string | null> {
    return todo("paletteSelectedItemText");
  }

  async paletteSearchResultsHeading(): Promise<string | null> {
    return todo("paletteSearchResultsHeading");
  }

  async isPaletteSearchHeadingPulsing(): Promise<boolean> {
    return todo("isPaletteSearchHeadingPulsing");
  }

  async paletteHasWorkspaceLevelToggle(): Promise<boolean> {
    return todo("paletteHasWorkspaceLevelToggle");
  }

  async isWorkspaceLevelToggleEnabled(): Promise<boolean> {
    return todo("isWorkspaceLevelToggleEnabled");
  }

  async toggleWorkspaceLevel(): Promise<void> {
    return todo("toggleWorkspaceLevel");
  }

  async countSearchRequests(_action: () => Promise<void>): Promise<number> {
    return todo("countSearchRequests");
  }

  async lastSearchRequestParams(): Promise<Record<string, string> | null> {
    return todo("lastSearchRequestParams");
  }

  async isShortcutsDialogOpen(): Promise<boolean> {
    return todo("isShortcutsDialogOpen");
  }

  async pressShortcutsDialogChord(): Promise<void> {
    return todo("pressShortcutsDialogChord");
  }

  async typeShortcutsFilter(_text: string): Promise<void> {
    return todo("typeShortcutsFilter");
  }

  async shortcutsDialogCommandTitles(): Promise<string[]> {
    return todo("shortcutsDialogCommandTitles");
  }

  async repoStarLinkAttributes(): Promise<{ href: string; target: string; rel: string } | null> {
    return todo("repoStarLinkAttributes");
  }

  async repoStarIconSrc(): Promise<string | null> {
    return todo("repoStarIconSrc");
  }

  async htmlClassList(): Promise<string> {
    return todo("htmlClassList");
  }

  async documentLang(): Promise<string> {
    return todo("documentLang");
  }

  async openBrowseWorkItem(_workspaceSlug: string, _identifier: string): Promise<void> {
    return todo("openBrowseWorkItem");
  }

  async browseShowsWorkItemDetail(): Promise<boolean> {
    return todo("browseShowsWorkItemDetail");
  }

  async browseShowsWorkspaceWideList(): Promise<boolean> {
    return todo("browseShowsWorkspaceWideList");
  }
}
