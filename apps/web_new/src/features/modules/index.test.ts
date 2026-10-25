// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  DEFAULT_MODULES_LAYOUT,
  defaultModuleLinkScheme,
  formatModuleDay,
  formatModuleOffsetChip,
  formatModuleRange,
  invalidateAfterModuleWrite,
  invalidateArchivedModuleReads,
  invalidateModuleLists,
  invalidateModuleReads,
  invalidateWorkspaceModules,
  MODULE_LINK_ADD_TITLE,
  MODULE_LINK_UPDATE_TITLE,
  MODULE_LINKS_EMPTY,
  MODULE_LINKS_TITLE,
  MODULE_ARCHIVE_GATE_HINT,
  MODULE_LINK_COPIED_BODY,
  MODULE_MENU_ARCHIVE,
  MODULE_MENU_COPY,
  MODULE_MENU_OPEN,
  MODULE_MENU_RESTORE,
  MODULE_OPEN_NEW_TAB_FAILED_TITLE,
  MODULE_RESTORE_SUCCESS_BODY,
  MODULE_RESTORE_SUCCESS_TITLE,
  MODULES_CRUMB,
  MODULES_LAYOUTS,
  MODULE_USER_ZONE_HEADING,
  moduleDateText,
  moduleDayAnchor,
  moduleDetailHref,
  moduleInstantReading,
  moduleLinkTitle,
  moduleOffsetChip,
  moduleProjectFactsQueryOptions,
  moduleRangeReading,
  moduleViewEnabled,
  moduleZoneOffsetMinutes,
  moduleKeys,
  modulesFeatureHref,
  modulesListHref,
  modulesListQueryOptions,
  modulesTitle,
  ModuleBoardCard,
  ModuleCreateAction,
  ModuleDeleteDialog,
  ModuleDetailActions,
  ModuleDialogProvider,
  ModuleForm,
  ModuleLayoutIcon,
  ModuleLinkDialog,
  ModuleLinksSection,
  ModuleSaveDialog,
  useModuleDialogs,
  ModuleListRow,
  ModuleRowMenu,
  ModulesFeatureGate,
  ModulesLayoutSwitcher,
  ModulesListBody,
  ModulesListHeader,
  ModulesListScreen,
  ModulesListSkeleton,
  ModulesTimeline,
  buildModuleTimelineRows,
  moduleTimelineTooltip,
  normalizeModuleTimelineRanks,
  parseModuleDay,
  useModuleTimelineMutations,
  warnOnTimelineReorderAttempt,
  isModuleGoneAway,
  moduleBadgeTooltip,
  moduleDetailTitle,
  MODULE_DETAIL_SIDEBAR_STORAGE_KEY,
  ModuleDetailBadge,
  ModuleDetailCrumbs,
  ModuleDetailGate,
  ModuleDetailScreen,
  ModuleGoneAway,
  MODULE_SIDEBAR_COLLAPSE_LABEL,
  MODULE_SIDEBAR_EXPAND_LABEL,
  ModuleSidebarToggle,
  parseSidebarCollapsed,
  projectIssuesHref,
  readSidebarCollapsed,
  resolveModulesListBody,
  sortModulesByName,
  switchModuleTarget,
  useDetailSidebarCollapsed,
  useModuleLinksWrite,
  userZoneDiffers,
  writeSidebarCollapsed,
  copyModuleLink,
  moduleCanArchive,
  moduleDetailMenuEntries,
  moduleMenuEntries,
  moduleRowHoverClass,
  openModuleInNewTab,
} from "./index.js";

describe("modules barrel", () => {
  it("re-exports the freshness helpers later slices call", () => {
    expect(moduleKeys).toBeDefined();
    expect(modulesListQueryOptions).toBeDefined();
    expect(invalidateModuleLists).toBeDefined();
    expect(invalidateModuleReads).toBeDefined();
    expect(invalidateArchivedModuleReads).toBeDefined();
    expect(invalidateWorkspaceModules).toBeDefined();
    expect(invalidateAfterModuleWrite).toBeDefined();
  });

  it("re-exports the list shell and its states", () => {
    expect(MODULES_CRUMB).toBe("Modules");
    expect(modulesTitle).toBeDefined();
    expect(modulesListHref).toBeDefined();
    expect(moduleDetailHref).toBeDefined();
    expect(modulesFeatureHref).toBeDefined();
    expect(resolveModulesListBody).toBeDefined();
    expect(sortModulesByName).toBeDefined();
    expect(ModulesListScreen).toBeDefined();
    expect(ModulesListHeader).toBeDefined();
    expect(ModulesFeatureGate).toBeDefined();
    expect(ModulesListSkeleton).toBeDefined();
    expect(ModulesListBody).toBeDefined();
    expect(ModulesLayoutSwitcher).toBeDefined();
    expect(ModuleLayoutIcon).toBeDefined();
    expect(ModuleListRow).toBeDefined();
    expect(ModuleBoardCard).toBeDefined();
    expect(ModulesTimeline).toBeDefined();
    expect(buildModuleTimelineRows).toBeDefined();
    expect(moduleTimelineTooltip).toBeDefined();
    expect(normalizeModuleTimelineRanks).toBeDefined();
    expect(warnOnTimelineReorderAttempt).toBeDefined();
    expect(useModuleTimelineMutations).toBeDefined();
  });

  it("re-exports the layout vocabulary and the shared date helpers", () => {
    expect(MODULES_LAYOUTS).toEqual(["list", "board", "timeline"]);
    expect(DEFAULT_MODULES_LAYOUT).toBe("list");
    expect(parseModuleDay).toBeDefined();
    expect(formatModuleDay).toBeDefined();
    expect(formatModuleRange).toBeDefined();
    expect(moduleDateText).toBeDefined();
    expect(userZoneDiffers).toBeDefined();
  });

  it("re-exports the project-zone date helpers later slices render with", () => {
    expect(MODULE_USER_ZONE_HEADING).toBe("In your timezone");
    expect(moduleZoneOffsetMinutes).toBeDefined();
    expect(formatModuleOffsetChip).toBeDefined();
    expect(moduleOffsetChip).toBeDefined();
    expect(moduleDayAnchor).toBeDefined();
    expect(moduleRangeReading).toBeDefined();
    expect(moduleInstantReading).toBeDefined();
  });

  it("re-exports the dialog openers every modules surface calls", () => {
    expect(useModuleDialogs).toBeDefined();
    expect(ModuleDialogProvider).toBeDefined();
    expect(ModuleCreateAction).toBeDefined();
    expect(ModuleSaveDialog).toBeDefined();
    expect(ModuleForm).toBeDefined();
    expect(ModuleDeleteDialog).toBeDefined();
    expect(ModuleDetailActions).toBeDefined();
  });

  it("re-exports the tolerant project facts read the gate uses", () => {
    expect(moduleProjectFactsQueryOptions).toBeDefined();
    expect(moduleViewEnabled).toBeDefined();
  });

  it("re-exports the detail frame and its sidebar collapse store", () => {
    expect(ModuleDetailScreen).toBeDefined();
    expect(ModuleDetailCrumbs).toBeDefined();
    expect(ModuleDetailGate).toBeDefined();
    expect(ModuleGoneAway).toBeDefined();
    expect(ModuleDetailBadge).toBeDefined();
    expect(ModuleSidebarToggle).toBeDefined();
    expect(MODULE_SIDEBAR_COLLAPSE_LABEL).toBe("Collapse sidebar");
    expect(MODULE_SIDEBAR_EXPAND_LABEL).toBe("Expand sidebar");
    expect(moduleDetailTitle).toBeDefined();
    expect(projectIssuesHref).toBeDefined();
    expect(switchModuleTarget).toBeDefined();
    expect(isModuleGoneAway).toBeDefined();
    expect(moduleBadgeTooltip).toBeDefined();
    expect(MODULE_DETAIL_SIDEBAR_STORAGE_KEY).toBe("module_detail_sidebar_collapsed");
    expect(parseSidebarCollapsed).toBeDefined();
    expect(readSidebarCollapsed).toBeDefined();
    expect(writeSidebarCollapsed).toBeDefined();
    expect(useDetailSidebarCollapsed).toBeDefined();
  });

  it("re-exports the shared quick menu later slices mount", () => {
    expect(ModuleRowMenu).toBeDefined();
    expect(moduleMenuEntries).toBeDefined();
    expect(moduleCanArchive).toBeDefined();
    expect(MODULE_MENU_OPEN).toBe("Open in new tab");
    expect(MODULE_MENU_COPY).toBe("Copy link");
    expect(MODULE_MENU_ARCHIVE).toBe("Archive");
    expect(MODULE_MENU_RESTORE).toBe("Restore");
    expect(MODULE_ARCHIVE_GATE_HINT).toContain("completed or cancelled");
    expect(MODULE_RESTORE_SUCCESS_TITLE).toBe("Restore success");
    expect(MODULE_RESTORE_SUCCESS_BODY).toContain("project modules");
    expect(moduleDetailMenuEntries).toBeDefined();
    expect(moduleRowHoverClass).toBeDefined();
    expect(copyModuleLink).toBeDefined();
    expect(openModuleInNewTab).toBeDefined();
    expect(MODULE_LINK_COPIED_BODY).toBe("Module link copied to clipboard.");
    expect(MODULE_OPEN_NEW_TAB_FAILED_TITLE).toBeDefined();
  });

  it("re-exports the sidebar links section later slices mount with", () => {
    expect(MODULE_LINKS_TITLE).toBe("Links");
    expect(MODULE_LINKS_EMPTY).toBe("No links added yet");
    expect(MODULE_LINK_ADD_TITLE).toBe("Add link");
    expect(MODULE_LINK_UPDATE_TITLE).toBe("Update link");
    expect(ModuleLinksSection).toBeDefined();
    expect(ModuleLinkDialog).toBeDefined();
    expect(defaultModuleLinkScheme).toBeDefined();
    expect(moduleLinkTitle).toBeDefined();
    expect(useModuleLinksWrite).toBeDefined();
  });
});
