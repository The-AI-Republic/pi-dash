// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// New-app driver skeleton (NEWFRONT-19). Implements the same interface as
// the oracle driver so scenarios compile against either target, but every
// action throws until the matching area lands in apps/web_new. Area issues
// fill these in method by method; the oracle driver stays untouched.
import type { Page } from "@playwright/test";
import type { ParityBrowserCookie, ParityDriver, ParityTarget } from "./parity-driver";

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

  // --- Projects list + lifecycle (NEWFRONT-124, SHELL-024..045) stubs. ---
  async openAuthenticatedAt(_path: string, _cookies: ParityBrowserCookie[]): Promise<void> {
    return todo("openAuthenticatedAt");
  }
  async currentUrlPath(): Promise<string> {
    return todo("currentUrlPath");
  }
  async hasText(_text: string): Promise<boolean> {
    return todo("hasText");
  }
  async openProjectsList(_workspaceSlug: string): Promise<void> {
    return todo("openProjectsList");
  }
  async openArchivedProjects(_workspaceSlug: string): Promise<void> {
    return todo("openArchivedProjects");
  }
  async visibleProjectCardNames(): Promise<string[]> {
    return todo("visibleProjectCardNames");
  }
  async awaitProjectCard(_name: string): Promise<void> {
    return todo("awaitProjectCard");
  }
  async gridColumnCount(): Promise<number> {
    return todo("gridColumnCount");
  }
  async setViewportWidth(_width: number): Promise<void> {
    return todo("setViewportWidth");
  }
  async isProjectsSkeletonVisible(): Promise<boolean> {
    return todo("isProjectsSkeletonVisible");
  }
  async emptyStateHeading(): Promise<string | null> {
    return todo("emptyStateHeading");
  }
  async isEmptyStateCreateVisible(): Promise<boolean> {
    return todo("isEmptyStateCreateVisible");
  }
  async isEmptyStateCreateEnabled(): Promise<boolean> {
    return todo("isEmptyStateCreateEnabled");
  }
  async clickEmptyStateCreate(): Promise<void> {
    return todo("clickEmptyStateCreate");
  }
  async isHeaderCreateButtonVisible(): Promise<boolean> {
    return todo("isHeaderCreateButtonVisible");
  }
  async headerCreateButtonLabel(): Promise<string | null> {
    return todo("headerCreateButtonLabel");
  }
  async clickHeaderCreateButton(): Promise<void> {
    return todo("clickHeaderCreateButton");
  }
  async breadcrumbLabels(): Promise<string[]> {
    return todo("breadcrumbLabels");
  }
  async isMobileListHeaderVisible(): Promise<boolean> {
    return todo("isMobileListHeaderVisible");
  }
  async isDesktopFilterRowVisible(): Promise<boolean> {
    return todo("isDesktopFilterRowVisible");
  }
  async breadcrumbTerminalIsLink(): Promise<boolean> {
    return todo("breadcrumbTerminalIsLink");
  }
  async openSortMenu(): Promise<void> {
    return todo("openSortMenu");
  }
  async selectSortOption(_label: string): Promise<void> {
    return todo("selectSortOption");
  }
  async currentSortLabel(): Promise<string> {
    return todo("currentSortLabel");
  }
  async isSortDirectionDisabled(): Promise<boolean> {
    return todo("isSortDirectionDisabled");
  }
  async closeMenu(): Promise<void> {
    return todo("closeMenu");
  }
  async openFilterMenu(): Promise<void> {
    return todo("openFilterMenu");
  }
  async typeFilterSearch(_text: string): Promise<void> {
    return todo("typeFilterSearch");
  }
  async filterMenuHasOption(_text: string): Promise<boolean> {
    return todo("filterMenuHasOption");
  }
  async selectFilterOption(_label: string): Promise<void> {
    return todo("selectFilterOption");
  }
  async isFilterBadgeVisible(): Promise<boolean> {
    return todo("isFilterBadgeVisible");
  }
  async appliedFilterChipTexts(): Promise<string[]> {
    return todo("appliedFilterChipTexts");
  }
  async removeAppliedFilterChip(_text: string): Promise<void> {
    return todo("removeAppliedFilterChip");
  }
  async clickClearAllFilters(): Promise<void> {
    return todo("clickClearAllFilters");
  }
  async filterMatchCountText(): Promise<string | null> {
    return todo("filterMatchCountText");
  }
  async openListSearch(): Promise<void> {
    return todo("openListSearch");
  }
  async typeListSearch(_text: string): Promise<void> {
    return todo("typeListSearch");
  }
  async listSearchValue(): Promise<string> {
    return todo("listSearchValue");
  }
  async isListSearchExpanded(): Promise<boolean> {
    return todo("isListSearchExpanded");
  }
  async pressEscapeInListSearch(): Promise<void> {
    return todo("pressEscapeInListSearch");
  }
  async clickListSearchClear(): Promise<void> {
    return todo("clickListSearchClear");
  }
  async clickOutsideListSearch(): Promise<void> {
    return todo("clickOutsideListSearch");
  }
  async cardShortCode(_name: string): Promise<string | null> {
    return todo("cardShortCode");
  }
  async cardHasPrivateMark(_name: string): Promise<boolean> {
    return todo("cardHasPrivateMark");
  }
  async cardSubText(_name: string): Promise<string | null> {
    return todo("cardSubText");
  }
  async cardHasFavoriteStar(_name: string): Promise<boolean> {
    return todo("cardHasFavoriteStar");
  }
  async clickFavoriteStar(_name: string): Promise<void> {
    return todo("clickFavoriteStar");
  }
  async clickProjectCard(_name: string): Promise<void> {
    return todo("clickProjectCard");
  }
  async openCardContextMenu(_name: string): Promise<void> {
    return todo("openCardContextMenu");
  }
  async contextMenuItemLabels(): Promise<string[]> {
    return todo("contextMenuItemLabels");
  }
  async clickContextMenuItem(_label: string): Promise<void> {
    return todo("clickContextMenuItem");
  }
  async cardFooterLabels(_name: string): Promise<string[]> {
    return todo("cardFooterLabels");
  }
  async clickCardJoin(_name: string): Promise<void> {
    return todo("clickCardJoin");
  }
  async isJoinDialogVisible(): Promise<boolean> {
    return todo("isJoinDialogVisible");
  }
  async joinDialogHeading(): Promise<string | null> {
    return todo("joinDialogHeading");
  }
  async confirmJoin(): Promise<void> {
    return todo("confirmJoin");
  }
  async openLeaveProjectDialog(_projectName: string): Promise<void> {
    return todo("openLeaveProjectDialog");
  }
  async fillLeaveProjectName(_text: string): Promise<void> {
    return todo("fillLeaveProjectName");
  }
  async fillLeaveConfirmPhrase(_text: string): Promise<void> {
    return todo("fillLeaveConfirmPhrase");
  }
  async submitLeave(): Promise<void> {
    return todo("submitLeave");
  }
  async leaveErrorText(): Promise<string | null> {
    return todo("leaveErrorText");
  }
  async isLeaveDialogVisible(): Promise<boolean> {
    return todo("isLeaveDialogVisible");
  }
  async openArchiveProjectDialog(_projectName: string): Promise<void> {
    return todo("openArchiveProjectDialog");
  }
  async archiveDialogBodyText(): Promise<string | null> {
    return todo("archiveDialogBodyText");
  }
  async confirmArchive(): Promise<void> {
    return todo("confirmArchive");
  }
  async clickCardRestore(_name: string): Promise<void> {
    return todo("clickCardRestore");
  }
  async confirmRestore(): Promise<void> {
    return todo("confirmRestore");
  }
  async archivedCardHasAdminActions(_name: string): Promise<boolean> {
    return todo("archivedCardHasAdminActions");
  }
  async cardShowsArchivedMarker(_name: string): Promise<boolean> {
    return todo("cardShowsArchivedMarker");
  }
  async openDeleteProjectDialog(_name: string): Promise<void> {
    return todo("openDeleteProjectDialog");
  }
  async fillDeleteProjectName(_text: string): Promise<void> {
    return todo("fillDeleteProjectName");
  }
  async fillDeleteConfirmPhrase(_text: string): Promise<void> {
    return todo("fillDeleteConfirmPhrase");
  }
  async isDeleteSubmitDisabled(): Promise<boolean> {
    return todo("isDeleteSubmitDisabled");
  }
  async submitDelete(): Promise<void> {
    return todo("submitDelete");
  }
  async isCreateProjectDialogVisible(): Promise<boolean> {
    return todo("isCreateProjectDialogVisible");
  }
  async fillCreateProjectName(_text: string): Promise<void> {
    return todo("fillCreateProjectName");
  }
  async createProjectShortCodeValue(): Promise<string> {
    return todo("createProjectShortCodeValue");
  }
  async fillCreateProjectShortCode(_text: string): Promise<void> {
    return todo("fillCreateProjectShortCode");
  }
  async submitCreateProject(): Promise<void> {
    return todo("submitCreateProject");
  }
  async createProjectErrorText(): Promise<string | null> {
    return todo("createProjectErrorText");
  }
}
