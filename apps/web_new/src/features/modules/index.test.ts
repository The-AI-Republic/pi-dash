// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  DEFAULT_MODULES_LAYOUT,
  formatModuleDay,
  formatModuleOffsetChip,
  formatModuleRange,
  invalidateAfterModuleWrite,
  invalidateArchivedModuleReads,
  invalidateModuleLists,
  invalidateModuleReads,
  invalidateWorkspaceModules,
  MODULES_CRUMB,
  MODULES_LAYOUTS,
  MODULE_USER_ZONE_HEADING,
  moduleDateText,
  moduleDayAnchor,
  moduleDetailHref,
  moduleInstantReading,
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
  ModuleLayoutIcon,
  ModuleListRow,
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
  userZoneDiffers,
  writeSidebarCollapsed,
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
});
