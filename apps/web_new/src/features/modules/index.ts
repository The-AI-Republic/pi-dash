// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Modules area barrel (parity/specs/modules.md §16). Slice A seeds it with
// the list route shell and its states plus the shared freshness, layout and
// date helpers every later slice reads; slices B–O extend it in place.
export {
  invalidateAfterModuleWrite,
  invalidateArchivedModuleReads,
  invalidateModuleLists,
  invalidateModuleReads,
  invalidateWorkspaceModules,
} from "./api/invalidate.js";
export { moduleKeys } from "./api/keys.js";
export { modulesListQueryOptions } from "./api/queries.js";
export { ModuleBoardCard, type ModuleBoardCardProps } from "./components/ModuleBoardCard.js";
export { ModuleLayoutIcon } from "./components/ModuleLayoutIcon.js";
export { ModuleListRow, type ModuleListRowProps } from "./components/ModuleListRow.js";
export { ModuleTimelineRow, type ModuleTimelineRowProps } from "./components/ModuleTimelineRow.js";
export { ModulesFeatureGate, type ModulesFeatureGateProps } from "./components/ModulesFeatureGate.js";
export { ModulesLayoutSwitcher, type ModulesLayoutSwitcherProps } from "./components/ModulesLayoutSwitcher.js";
export { ModulesListBody, type ModulesListBodyProps } from "./components/ModulesListBody.js";
export { ModulesListHeader, type ModulesListHeaderProps } from "./components/ModulesListHeader.js";
export { ModulesListScreen, type ModulesListScreenProps } from "./components/ModulesListScreen.js";
export { ModulesListSkeleton, type ModulesListSkeletonProps } from "./components/ModulesListSkeleton.js";
export {
  MODULES_LIST_PREFS_STORAGE_KEY,
  parseLayoutRecord,
  readLayoutRecord,
  readProjectLayout,
  useModuleLayout,
  writeProjectLayout,
  type ModuleLayoutHandle,
  type ModulesLayoutRecord,
  type StorageLike,
} from "./lib/listLayoutStore.js";
export {
  compareModuleNames,
  MODULES_CRUMB,
  moduleDetailHref,
  moduleIssueCountText,
  moduleProgressPercent,
  moduleProgressRatio,
  modulesFeatureHref,
  modulesListHref,
  modulesTitle,
  resolveModulesListBody,
  sortModulesByName,
  type ModulesListBodyInput,
  type ModulesListBodyState,
} from "./lib/moduleList.js";
export {
  DEFAULT_MODULES_LAYOUT,
  isModulesLayout,
  MODULES_COMPACT_LAYOUT_OPTIONS,
  MODULES_LAYOUT_LABELS,
  MODULES_LAYOUTS,
  parseModulesLayout,
  resolveCompactLayout,
  type ModulesCompactLayout,
  type ModulesLayout,
} from "./lib/moduleLayouts.js";
export {
  formatModuleDay,
  formatModuleRange,
  moduleDateText,
  moduleRangeDurationDays,
  parseModuleDay,
  userZoneDiffers,
  type ModuleDateRange,
  type ModuleZoneDay,
} from "./lib/moduleDates.js";
