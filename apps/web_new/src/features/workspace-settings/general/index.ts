// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the workspace-settings general page (WSET-003–006).

export { WorkspaceGeneralPage, copyWorkspaceAddress } from "./WorkspaceGeneralPage.js";
export type { WorkspaceGeneralPageProps } from "./WorkspaceGeneralPage.js";
export { WorkspaceIdentityCard } from "./components/WorkspaceIdentityCard.js";
export type { WorkspaceIdentityCardProps } from "./components/WorkspaceIdentityCard.js";
export { DETAILS_SAVE_LABEL, DETAILS_SAVING_LABEL, WorkspaceDetailsForm } from "./components/WorkspaceDetailsForm.js";
export type { StoredDetails, WorkspaceDetailsFormProps } from "./components/WorkspaceDetailsForm.js";
export { LOGO_DIALOG_HINT, LOGO_DIALOG_TITLE, WorkspaceLogoDialog } from "./components/WorkspaceLogoDialog.js";
export type { WorkspaceLogoDialogProps } from "./components/WorkspaceLogoDialog.js";
export {
  DELETE_DIALOG_TITLE,
  DELETE_SECTION_DESCRIPTION,
  DELETE_SECTION_TITLE,
  DeleteWorkspaceSection,
} from "./components/DeleteWorkspaceSection.js";
export type { DeleteWorkspaceSectionProps } from "./components/DeleteWorkspaceSection.js";
export {
  DETAILS_SAVED_MESSAGE,
  DETAILS_SAVED_TITLE,
  LOGO_REMOVE_FAILED_MESSAGE,
  LOGO_REMOVE_FAILED_TITLE,
  LOGO_REMOVED_MESSAGE,
  LOGO_REMOVED_TITLE,
  LOGO_UPLOAD_FAILED_MESSAGE,
  LOGO_UPLOAD_FAILED_TITLE,
  URL_COPIED_TITLE,
  WORKSPACE_DELETED_TITLE,
  WORKSPACE_DELETE_FAILED_MESSAGE,
  WORKSPACE_DELETE_FAILED_TITLE,
  applyLogoToDetails,
  detailsUpdateInput,
  useDeleteWorkspace,
  useRemoveWorkspaceLogo,
  useSaveWorkspaceDetails,
  useUploadWorkspaceLogo,
} from "./api/mutations.js";
export type { LogoRemoveInput, LogoUploadInput } from "./api/mutations.js";
export { timezoneKeys, timezoneListQueryOptions, useTimezoneList } from "./api/timezones.js";
export { workspaceDetailsKey, workspaceDetailsQueryOptions } from "./api/details.js";
export {
  assetKeyFromUrl,
  canConfirmWorkspaceDelete,
  DELETE_CONFIRM_PHRASE,
  isLegacyLogoValue,
  LOGO_ACCEPTED_TYPES,
  LOGO_ACCEPT_ATTRIBUTE,
  LOGO_MAX_BYTES,
  LOGO_TOO_LARGE_MESSAGE,
  LOGO_WRONG_TYPE_MESSAGE,
  mergeTimezoneOptions,
  ORGANIZATION_SIZE_BANDS,
  ORGANIZATION_SIZE_PLACEHOLDER,
  postDeletePath,
  timezoneChoiceLabel,
  validateLogoFile,
  validateWorkspaceDetailsName,
  WORKSPACE_NAME_MAX_LENGTH,
  workspaceAddress,
  workspaceInitial,
} from "./rules.js";
export type { TimezoneChoice, TimezoneRow } from "./rules.js";
