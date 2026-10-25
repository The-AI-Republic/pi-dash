// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Modules area barrel (parity/specs/modules.md §16). Slice A seeds it with
// the list route shell and its states plus the shared freshness, layout and
// date helpers every later slice reads; slice B adds the header controls
// (search, ordering, filters, chips, no-match) and their pure model;
// slices C–O extend it in place.
export {
  invalidateAfterModuleWrite,
  invalidateArchivedModuleReads,
  invalidateModuleLists,
  invalidateModuleReads,
  invalidateWorkspaceModules,
} from "./api/invalidate.js";
export { moduleKeys } from "./api/keys.js";
export {
  getModuleProjectFacts,
  ModuleProjectFacts,
  moduleProjectFactsQueryOptions,
  moduleProjectPath,
  moduleProjectTimeZone,
  moduleViewEnabled,
} from "./api/projectFacts.js";
export { modulesListQueryOptions } from "./api/queries.js";
export { ModuleBoardCard, type ModuleCardLead, type ModuleBoardCardProps } from "./components/ModuleBoardCard.js";
export { ModulePeekPanel, modulePeekSummary, type ModulePeekPanelProps } from "./components/ModulePeekPanel.js";
export {
  ModuleSegmentedBar,
  moduleSegmentSummary,
  type ModuleSegmentedBarProps,
} from "./components/ModuleSegmentedBar.js";
export { ModuleFeatureSettings, type ModuleFeatureSettingsProps } from "./components/ModuleFeatureSettings.js";
export {
  ModuleCreateAction,
  ModuleDeleteDialog,
  ModuleDetailActions,
  ModuleDialogProvider,
  ModuleForm,
  ModuleSaveDialog,
  MODULE_ADD_LABEL,
  MODULE_ADD_LABEL_COMPACT,
  MODULE_MENU_DELETE,
  MODULE_MENU_EDIT,
  useModuleDialogs,
  type ModuleCreateActionProps,
  type ModuleDeleteDialogProps,
  type ModuleDeleteOptions,
  type ModuleDeleteTarget,
  type ModuleDetailActionsProps,
  type ModuleDialogHandle,
  type ModuleDialogProviderProps,
  type ModuleFormProject,
  type ModuleFormProps,
  type ModuleSaveDialogProps,
} from "./components/dialogs/index.js";
export {
  ModuleListProvider,
  useModuleListContext,
  type ModuleListContextValue,
  type ModuleListProviderProps,
} from "./components/moduleListContext.js";
export { ModulesFilterChips, type ModulesFilterChipsProps } from "./components/ModulesFilterChips.js";
export {
  MODULE_FILTER_OPTION_CAP,
  ModulesFilterMenu,
  type ModulesFilterMenuProps,
} from "./components/ModulesFilterMenu.js";
export { MODULES_NO_MATCH_HEADING, MODULES_NO_MATCH_HINT, ModulesNoMatch } from "./components/ModulesNoMatch.js";
export { MODULES_SEARCH_TRIGGER_LABEL, ModulesSearch, type ModulesSearchProps } from "./components/ModulesSearch.js";
export { ModulesSortMenu, type ModulesSortMenuProps } from "./components/ModulesSortMenu.js";
export { ModuleLayoutIcon } from "./components/ModuleLayoutIcon.js";
export { ModuleListRow, moduleRowHoverClass, type ModuleListRowProps } from "./components/ModuleListRow.js";
export { moduleDetailMenuEntries } from "./components/menus/moduleDetailMenuEntries.js";
export {
  MODULE_ARCHIVE_GATE_HINT,
  MODULE_MENU_ARCHIVE,
  MODULE_MENU_COPY,
  MODULE_MENU_OPEN,
  MODULE_MENU_RESTORE,
  moduleCanArchive,
  moduleMenuEntries,
  type ModuleMenuAssembly,
  type ModuleMenuEntryId,
  type ModuleMenuEntryPlan,
} from "./components/menus/moduleMenuModel.js";
export {
  MODULE_RESTORE_SUCCESS_BODY,
  MODULE_RESTORE_SUCCESS_TITLE,
  ModuleRowMenu,
  type ModuleRowMenuProps,
} from "./components/menus/ModuleRowMenu.js";
export {
  buildModuleCreateCommand,
  buildModuleJumpCommand,
  MODULE_CREATE_COMMAND_ID,
  MODULE_CREATE_SEQUENCE,
  MODULE_CREATE_TITLE,
  MODULE_JUMP_COMMAND_ID,
  MODULE_JUMP_PAGE_ID,
  MODULE_JUMP_TITLE,
  type ModuleCreateCommandContext,
  type ModuleJumpCommandContext,
} from "./palette/moduleCommands.js";
export {
  MODULE_JUMP_EMPTY_TEXT,
  MODULE_JUMP_PLACEHOLDER,
  projectModulesJumpPage,
  type ModuleJumpPageArgs,
} from "./palette/moduleJumpPage.js";
export { ModulePaletteEntries, type ModulePaletteEntriesProps } from "./palette/ModulePaletteEntries.js";
export { ModuleStatusDot, type ModuleStatusDotProps } from "./palette/ModuleStatusDot.js";
export { ModulesFeatureGate, type ModulesFeatureGateProps } from "./components/ModulesFeatureGate.js";
export {
  MODULE_TIMELINE_REORDER_FAILED_BODY,
  MODULE_TIMELINE_REORDER_FAILED_TITLE,
  MODULE_TIMELINE_REORDER_WARNING_BODY,
  MODULE_TIMELINE_REORDER_WARNING_TITLE,
  buildModuleTimelineRows,
  changedModuleTimelineRanks,
  moduleTimelineBarColors,
  moduleTimelineBarTitle,
  moduleTimelinePrefixStatus,
  moduleTimelineRange,
  moduleTimelineTooltip,
  moduleTimelineTooltipDay,
  moduleTimelineTooltipForId,
  normalizeModuleTimelineRanks,
  warnOnTimelineReorderAttempt,
  withModuleTimelineDates,
  withModuleTimelineRanks,
  type ModuleTimelineRange,
  type ModuleTimelineRankWrite,
} from "./components/timeline/moduleTimelineModel.js";
export { ModulesTimeline, type ModulesTimelineProps } from "./components/timeline/ModulesTimeline.js";
export {
  useModuleTimelineMutations,
  type ModuleTimelineMutations,
  type ModuleTimelineMutationsArgs,
} from "./components/timeline/useModuleTimelineMutations.js";
export { ModulesLayoutSwitcher, type ModulesLayoutSwitcherProps } from "./components/ModulesLayoutSwitcher.js";
export { ModulesListBody, type ModulesListBodyProps } from "./components/ModulesListBody.js";
export { ModulesListHeader, type ModulesListHeaderProps } from "./components/ModulesListHeader.js";
export { ModulesListScreen, type ModulesListScreenProps } from "./components/ModulesListScreen.js";
export { ModulesListSkeleton, type ModulesListSkeletonProps } from "./components/ModulesListSkeleton.js";
export { ModuleAddWorkItem, type ModuleAddWorkItemProps } from "./components/ModuleAddWorkItem.js";
export { ModuleAddWorkItemButton, type ModuleAddWorkItemButtonProps } from "./components/ModuleAddWorkItemButton.js";
export {
  MODULE_ADD_EXISTING_FAILURE,
  MODULE_ADD_EXISTING_SUCCESS,
  MODULE_ADD_WORK_ITEM_LABEL,
  MODULE_PICKER_SEARCH_HINT,
  canAddWorkItemsToModule,
  type ModuleAddStanding,
} from "./lib/moduleAddWorkItem.js";
export {
  useModuleAddWorkItem,
  type ModuleAddWorkItemController,
  type UseModuleAddWorkItemInput,
} from "./lib/useModuleAddWorkItem.js";
export {
  useModuleAddExisting,
  type ModuleAddExisting,
  type UseModuleAddExistingInput,
} from "./lib/useModuleAddExisting.js";
export { ModuleDetailBadge, type ModuleDetailBadgeProps } from "./components/ModuleDetailBadge.js";
export { ModuleDetailCrumbs, type ModuleDetailCrumbsProps } from "./components/ModuleDetailCrumbs.js";
export { ModuleDetailGate, type ModuleDetailGateProps } from "./components/ModuleDetailGate.js";
export { ModuleDetailScreen, type ModuleDetailScreenProps } from "./components/ModuleDetailScreen.js";
export { ModuleGoneAway, type ModuleGoneAwayProps } from "./components/ModuleGoneAway.js";
export {
  MODULE_SIDEBAR_COLLAPSE_LABEL,
  MODULE_SIDEBAR_EXPAND_LABEL,
  ModuleSidebarToggle,
  type ModuleSidebarToggleProps,
} from "./components/ModuleSidebarToggle.js";
export {
  isModuleGoneAway,
  moduleBadgeTooltip,
  moduleDetailTitle,
  projectIssuesHref,
  switchModuleTarget,
} from "./lib/moduleDetail.js";
export {
  MODULE_DETAIL_SIDEBAR_STORAGE_KEY,
  parseSidebarCollapsed,
  readSidebarCollapsed,
  useDetailSidebarCollapsed,
  writeSidebarCollapsed,
  type DetailSidebarHandle,
} from "./lib/detailSidebarStore.js";
export {
  MODULE_LINK_ADD_TITLE,
  MODULE_LINK_COPIED,
  MODULE_LINK_CREATED,
  MODULE_LINK_DELETED,
  MODULE_LINK_UPDATED,
  MODULE_LINK_UPDATE_TITLE,
  MODULE_LINKS_EMPTY,
  MODULE_LINKS_TITLE,
  MODULE_SIDEBAR_LOAD_FAILED,
  MODULE_STATUS_LABELS,
  ModuleAnalyticsSidebar,
  ModuleAnalyticsSidebarContent,
  ModuleAnalyticsSidebarSkeleton,
  ModuleLinkDialog,
  ModuleLinksSection,
  defaultModuleLinkScheme,
  describeModuleLinkMeta,
  moduleCountText,
  moduleLinkFavicon,
  moduleLinkTitle,
  moduleLinksNeedAuthors,
  moduleSidebarEditable,
  ModuleProgressSection,
  ModuleProgressSectionContent,
  resolveModuleLinkAuthor,
  showsModulePoints,
  useModuleLinksWrite,
  type ModuleAnalyticsSidebarProps,
  type ModuleLinkDialogProps,
  type ModuleLinkDraft,
  type ModuleLinkSummary,
  type ModuleLinkUpdate,
  type ModuleLinksSectionProps,
  type ModuleLinksWriteArgs,
  type ModuleProgressSectionContentProps,
  type ModuleProgressSectionProps,
} from "./components/sidebar/index.js";
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
  localIsoDay,
  clearModuleDateRange,
  clearModuleFilterDimension,
  clearModuleFilters,
  compareModuleNaturalNames,
  compareModulesBySort,
  DEFAULT_MODULE_SORT,
  defaultModuleCustomRange,
  emptyModuleFilters,
  hasModuleFilters,
  isModuleFilterDimensionActive,
  isModuleSortDirection,
  isModuleSortKey,
  matchesModuleMenuQuery,
  MODULE_DATE_CUSTOM_LABEL,
  MODULE_DATE_PRESETS,
  MODULE_FILTER_CHIP_KEYS,
  MODULE_FILTER_DIMENSIONS,
  MODULE_FILTER_LABELS,
  MODULE_SORT_DIRECTION_LABELS,
  MODULE_SORT_DIRECTIONS,
  MODULE_SORT_KEYS,
  MODULE_SORT_LABELS,
  MODULE_SORT_OPTIONS,
  MODULE_STATUS_OPTIONS,
  moduleDatePresetDay,
  moduleDateRange,
  moduleDateTokenKey,
  moduleListContentState,
  moduleMatchesDateDimension,
  moduleMatchesFilters,
  moduleMatchesSearch,
  moduleMemberName,
  moduleProgressShare,
  moduleSortHasDirection,
  moduleStatusColor,
  moduleStatusLabel,
  orderModules,
  removeModuleFilterValue,
  searchModules,
  selectModuleChipGroups,
  selectModuleMemberOptions,
  selectModuleSort,
  toggleModuleDateToken,
  toggleModuleFilterValue,
  visibleModules,
  type ModuleChipInput,
  type ModuleChipVisibility,
  type ModuleDatePreset,
  type ModuleDateToken,
  type ModuleDateTokens,
  type ModuleFilterChipGroup,
  type ModuleFilterChipValue,
  type ModuleFilterDimension,
  type ModuleListContentState,
  type ModuleListFilters,
  type ModuleMemberOption,
  type ModuleMemberOptionInput,
  type ModuleSort,
  type ModuleSortDirection,
  type ModuleSortKey,
} from "./lib/moduleListControls.js";
export {
  MODULES_LIST_DISPLAY_STORAGE_KEY,
  currentProjectDisplay,
  parseDisplayRecord,
  readDisplayRecord,
  readProjectDisplay,
  resetModuleDisplayCache,
  useModuleListDisplay,
  writeProjectDisplay,
  type ModuleListDisplay,
  type ModuleListDisplayHandle,
  type ModuleListDisplayRecord,
} from "./lib/moduleListDisplayStore.js";
export {
  MODULE_SEGMENT_COLORS,
  MODULE_SEGMENT_GROUPS,
  MODULE_SEGMENT_LABELS,
  moduleLeadSlot,
  modulePercentFloor,
  moduleProgressMarker,
  moduleProgressSegments,
  moduleWorkItemCountText,
  type ModuleLeadSlot,
  type ModuleProgressMarker,
  type ModuleProgressSegment,
  type ModuleStateSegmentGroup,
} from "./lib/moduleIdentity.js";
export { parseModulesListSearch, type ModulesListSearch } from "./lib/moduleListSearch.js";
export { moduleBoardGridClass, modulePeekFromSearch, withModulePeek } from "./lib/modulePeek.js";
export { modulePressHitControl } from "./lib/modulePress.js";
export {
  copyModuleLink,
  MODULE_LINK_COPIED_BODY,
  MODULE_OPEN_NEW_TAB_FAILED_TITLE,
  openModuleInNewTab,
} from "./lib/moduleShare.js";
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
  formatModuleOffsetChip,
  formatModuleRange,
  MODULE_USER_ZONE_HEADING,
  moduleDateText,
  moduleDayAnchor,
  moduleInstantDay,
  moduleInstantReading,
  moduleOffsetChip,
  moduleRangeDurationDays,
  moduleRangeReading,
  moduleZoneOffsetMinutes,
  parseModuleDay,
  userZoneDiffers,
  type ModuleDateRange,
  type ModuleInstantReading,
  type ModuleZoneDay,
  type ModuleZoneReading,
} from "./lib/moduleDates.js";
