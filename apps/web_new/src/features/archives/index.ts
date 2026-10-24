// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the archives feature. Other features and routes import
// only from here. Tab slices mount their content into the exported
// shell and slot; the detail slice mounts into the detail frame.

export { archivesKeys } from "./api/keys.js";
export {
  archivedCycleDetailQueryOptions,
  archivedCycleStoreSlot,
  useArchivedCycleDetail,
  useArchivedCycleListFilter,
} from "./api/archivedCycles.js";
export type { ArchivedCycleListFilter } from "./api/archivedCycles.js";
export { archivedCountQueryOptions, useArchivedIssuesCount } from "./api/count.js";
export { useArchivesProject } from "./api/project.js";
export type { ArchivesProject } from "./api/project.js";
export { ArchivedCyclePeekPanel } from "./components/ArchivedCyclePeekPanel.js";
export type { ArchivedCyclePeekPanelProps } from "./components/ArchivedCyclePeekPanel.js";
export { ARCHIVED_CYCLES_EMPTY_HEADING, ArchivedCyclesEmpty } from "./components/ArchivedCyclesEmpty.js";
export { ArchivedCyclesScreen } from "./components/ArchivedCyclesScreen.js";
export type { ArchivedCyclesScreenProps } from "./components/ArchivedCyclesScreen.js";
export { ARCHIVED_CYCLES_SEARCH_LABEL, ArchivedCyclesSearchBox } from "./components/ArchivedCyclesSearchBox.js";
export type { ArchivedCyclesSearchBoxProps } from "./components/ArchivedCyclesSearchBox.js";
export { ArchivedCyclesSkeleton } from "./components/ArchivedCyclesSkeleton.js";
export { ArchivesCountBadge } from "./components/ArchivesCountBadge.js";
export type { ArchivesCountBadgeProps } from "./components/ArchivesCountBadge.js";
export { ArchivesDetailFrame } from "./components/ArchivesDetailFrame.js";
export type { ArchivesDetailFrameProps } from "./components/ArchivesDetailFrame.js";
export { ArchivesHeader } from "./components/ArchivesHeader.js";
export type { ArchivesHeaderProps } from "./components/ArchivesHeader.js";
export { ArchivesShell } from "./components/ArchivesShell.js";
export type { ArchivesShellProps } from "./components/ArchivesShell.js";
export { ArchivesTabSlot } from "./components/ArchivesTabSlot.js";
export type { ArchivesTabSlotProps } from "./components/ArchivesTabSlot.js";
export { ArchivesTabStrip } from "./components/ArchivesTabStrip.js";
export type { ArchivesTabStripProps } from "./components/ArchivesTabStrip.js";
export { resolveArchivedCyclesBody } from "./lib/archivedCyclesBody.js";
export type { ArchivedCyclesBodyInput, ArchivedCyclesBodyState } from "./lib/archivedCyclesBody.js";
export { ARCHIVES_BACK_BREAKPOINT_PX, useArchivesBackVisible } from "./lib/narrow.js";
export {
  ARCHIVES_HEAD_FALLBACK,
  ARCHIVES_TAB_DEFS,
  archivesRouteTitle,
  archivesTabDef,
  archivesTabHref,
  archivesTabTitle,
  selectArchivesTabs,
} from "./tabs.js";
export type { ArchivesTab, ArchivesTabDef, ArchivesTabFlags, ArchivesTabKey } from "./tabs.js";
