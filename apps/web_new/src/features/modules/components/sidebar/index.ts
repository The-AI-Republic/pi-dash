// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module analytics sidebar exports (MOD-033). Feature code imports the
// sidebar through the modules barrel; these exports exist for the barrel
// and focused tests.

export {
  MODULE_SIDEBAR_LOAD_FAILED,
  ModuleAnalyticsSidebar,
  ModuleAnalyticsSidebarContent,
  ModuleAnalyticsSidebarSkeleton,
} from "./ModuleAnalyticsSidebar.js";
export type { ModuleAnalyticsSidebarProps } from "./ModuleAnalyticsSidebar.js";
export { MODULE_NAME_EMPTY, ModuleDescriptionField, ModuleNameField } from "./ModuleIdentityFields.js";
export type { ModuleDescriptionFieldProps, ModuleNameFieldProps } from "./ModuleIdentityFields.js";
export { ModuleLinkDialog } from "./ModuleLinkDialog.js";
export type { ModuleLinkDialogProps, ModuleLinkDraft } from "./ModuleLinkDialog.js";
export { ModuleLinksSection } from "./ModuleLinksSection.js";
export type { ModuleLinksSectionProps } from "./ModuleLinksSection.js";
export {
  MODULE_LINK_ADD_TITLE,
  MODULE_LINK_COPIED,
  MODULE_LINK_CREATED,
  MODULE_LINK_DELETED,
  MODULE_LINK_UPDATED,
  MODULE_LINK_UPDATE_TITLE,
  MODULE_LINKS_EMPTY,
  MODULE_LINKS_TITLE,
  defaultModuleLinkScheme,
  describeModuleLinkMeta,
  moduleLinkFavicon,
  moduleLinkTitle,
  moduleLinksNeedAuthors,
  resolveModuleLinkAuthor,
} from "./moduleLinksModel.js";
export type { ModuleLinkSummary } from "./moduleLinksModel.js";
export { useModuleLinksWrite } from "./useModuleLinksWrite.js";
export type { ModuleLinkUpdate, ModuleLinksWriteArgs } from "./useModuleLinksWrite.js";
export { MODULE_STATUS_TONES, ModuleStatusPicker } from "./ModuleStatusPicker.js";
export type { ModuleStatusPickerProps } from "./ModuleStatusPicker.js";
export {
  MODULE_SAVE_FAILED_BODY,
  MODULE_SAVE_SUCCESS,
  MODULE_STATUS_LABELS,
  moduleCountText,
  moduleSidebarEditable,
  showsModulePoints,
} from "./moduleSidebarModel.js";
export {
  archivedModuleDetailQueryOptions,
  moduleDetailQueryOptions,
  moduleSidebarDetailQueryOptions,
  moduleSidebarEstimatesQueryOptions,
} from "./moduleSidebarQueries.js";
export type { ModuleSidebarRecord } from "./moduleSidebarQueries.js";
export { MODULE_SAVE_FAILED_TITLE, moduleSaveFailureBody, useModuleSidebarWrite } from "./useModuleSidebarWrite.js";
export type { ModuleSidebarWriteArgs } from "./useModuleSidebarWrite.js";
