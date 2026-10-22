// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Instance-admin console (god-mode) feature barrel. Other features and
// routes reach admin code only through here.

export { GOD_MODE_ENTRY, resolveSetupError, resolveSignInBanners } from "./adminErrors.js";
export type { SetupError, SetupErrorPlacement, SignInBanners, SignInMappedBanner } from "./adminErrors.js";
export { decideAdminGuard, GOD_MODE_GENERAL } from "./adminGuards.js";
export type { AdminGuardDecision, AdminGuardTree } from "./adminGuards.js";
export { auditAdminAbsence, auditAdminToggles } from "./adminInvariants.js";
export type { AdminAbsenceAudit, AdminToggleAudit } from "./adminInvariants.js";
export {
  ADMIN_NARROW_VIEWPORT_PX,
  ADMIN_SEGMENT_LABELS,
  ADMIN_SIDEBAR_COLLAPSED_KEY,
  ADMIN_SIDEBAR_ENTRIES,
  buildAdminCrumbs,
  isSidebarEntryActive,
} from "./adminShell.js";
export type { AdminCrumb, AdminSidebarEntryDef } from "./adminShell.js";
export { formatConfigFlag, parseConfigFlag } from "./emailConfig.js";
export type { ConfigValueMap, EmailFormValues, EmailSecurityChoice } from "./emailConfig.js";
export {
  AI_KEY_DOCS_URL,
  AI_MODEL_DOCS_URL,
  AI_SETTINGS_SAVED_MESSAGE,
  AI_SETTINGS_SAVED_TITLE,
  AI_VENDOR_CONTACT_URL,
  buildAiSettingsPatch,
  LLM_API_KEY,
  LLM_MODEL_KEY,
  selectAiSettings,
  useSaveAiSettings,
} from "./aiSettings.js";
export type { AiSettingsValues } from "./aiSettings.js";
export {
  AUTH_METHOD_ENABLE_KEYS,
  AUTH_METHODS,
  canDisableAuthMethod,
  ENABLE_SIGNUP_KEY,
  isConfigFlagOn,
  isMethodConfigured,
  toConfigMap,
} from "./authMethods.js";
export type { AuthMethodDef, AuthMethodKey, ConfigMap } from "./authMethods.js";
export {
  AUTH_PROVIDER_BY_SLUG,
  AUTH_PROVIDER_LANDING_HREF,
  AUTH_PROVIDER_SKELETON_BARS,
  AUTH_PROVIDERS,
  buildAuthCallbackUrl,
  buildProviderCopyFields,
  buildProviderFormDefaults,
  buildProviderSavePayload,
  decodeSyncFlag,
  draftValue,
  encodeSyncFlag,
  isProviderFormDirty,
  isProviderFormSubmittable,
  PROVIDER_BACK_LABEL,
  PROVIDER_COPY_DONE_TITLE,
  PROVIDER_DISCARD_BODY,
  PROVIDER_DISCARD_KEEP_LABEL,
  PROVIDER_DISCARD_TITLE,
  PROVIDER_MASTER_FAILED_MESSAGE,
  PROVIDER_MASTER_FAILED_TITLE,
  PROVIDER_MASTER_SAVED_TITLE,
  PROVIDER_SAVE_BUSY_LABEL,
  PROVIDER_SAVE_DONE_TITLE,
  PROVIDER_SAVE_IDLE_LABEL,
  providerCopyDoneMessage,
  providerMasterSavedMessage,
  providerSaveDoneMessage,
  readProviderSavedValues,
  resolveAuthOrigin,
  resolveCopyHelpHref,
} from "./providerAuth.js";
export type {
  AuthProviderDef,
  AuthProviderSlug,
  CopyHelpHref,
  ProviderCopyField,
  ProviderCredentialField,
  ProviderFormValues,
} from "./providerAuth.js";
export { PROVIDER_CREDENTIALS } from "./providerCredentials.js";
export type { ProviderConfigCopyHelp, ProviderCredentialConfig } from "./providerCredentials.js";
export {
  CONFIG_SAVE_FAILED_LOG,
  createInstanceWorkspaceMutationOptions,
  createLoopJobMutationOptions,
  deleteLoopJobMutationOptions,
  toggleLoopJobMutationOptions,
  configValueMap,
  updateLoopJobMutationOptions,
  useAdminCsrfToken,
  useAdminSession,
  useCreateInstanceWorkspace,
  useCreateLoopJob,
  useDeleteLoopJob,
  useInstanceAdmins,
  useInstanceBootstrap,
  useInstanceConfigurations,
  useInstanceRunners,
  useInstanceWorkspaces,
  useLoopJobDetail,
  useLoopJobs,
  useLoopJobTargets,
  useRestrictWorkspaceCreation,
  useToggleLoopJob,
  useUpdateGeneralSettings,
  useUpdateInstanceConfigurations,
  useUpdateLoopJob,
} from "./hooks.js";
export type { LoopJobToggle, LoopJobUpdate } from "./hooks.js";
export {
  firstAdminEmail,
  GENERAL_NAME_REQUIRED_MESSAGE,
  GENERAL_SAVE_BUSY_LABEL,
  GENERAL_SAVE_FAILURE_LOG,
  GENERAL_SAVE_IDLE_LABEL,
  GENERAL_SAVE_SUCCESS_MESSAGE,
  GENERAL_SAVE_SUCCESS_TITLE,
  GENERAL_TELEMETRY_DOC_URL,
  isGeneralNameValid,
  mergeGeneralSettingsIntoBootstrap,
  resolveTelemetryPrefill,
} from "./generalSettings.js";
export { LOOP_MIN_ROLE_LABELS, resolveLoopMinRoleLabel } from "./loopJobs.js";
export {
  adminCsrfQueryOptions,
  adminKeys,
  adminQueryDefaults,
  adminSessionQueryOptions,
  instanceAdminsQueryOptions,
  instanceBootstrapQueryOptions,
  instanceConfigurationsQueryOptions,
  instanceRunnersQueryOptions,
  instanceWorkspacesQueryOptions,
  loopJobDetailQueryOptions,
  loopJobsQueryOptions,
  loopJobTargetsQueryOptions,
  mergeConfigurationRows,
  selectNextWorkspaceCursor,
} from "./queries.js";
export {
  acceptCompanyName,
  acceptPersonName,
  COMPANY_NAME_MAX_LENGTH,
  isCompanyNameText,
  isMismatchVisible,
  isPersonNameText,
  isSetupSubmittable,
  isStrengthMeterVisible,
  PERSON_NAME_MAX_LENGTH,
} from "./setupValidation.js";
export type { MismatchInput, SetupGateInput } from "./setupValidation.js";
export {
  registerAdminStoreReset,
  resetAdminClientState,
  resetAdminStores,
  useAdminSidebarStore,
  useFirstWorkspaceStore,
  useWorkspaceCreateDraftStore,
  WORKSPACE_CREATE_BLANK_DRAFT,
} from "./stores.js";
export type { AdminStoreReset, WorkspaceCreateDraft } from "./stores.js";
export {
  ADMIN_THEME_KEY,
  applyAdminThemePreference,
  applyStoredAdminTheme,
  readStoredAdminTheme,
  useAdminThemeStore,
} from "./theme.js";
export {
  CONFIG_SAVE_FAILURE_MESSAGE,
  CONFIG_SAVE_FAILURE_TITLE,
  CONFIG_SAVE_PROGRESS_TITLE,
  CONFIG_SAVE_SUCCESS_MESSAGE,
  CONFIG_SAVE_SUCCESS_TITLE,
  DISABLE_WORKSPACE_CREATION_KEY,
  encodeWorkspaceCreationRestricted,
  isWorkspaceCreationRestricted,
  selectConfigurationValue,
} from "./workspace.js";
export {
  buildWorkspaceCreateInput,
  deriveWorkspaceSlug,
  formatWorkspaceSlugPrefix,
  isRestrictedWorkspaceSlug,
  isWorkspaceCreateSubmittable,
  ORGANIZATION_SIZE_BANDS,
  shouldShowInvalidSlugHint,
  validateWorkspaceCreateName,
  validateWorkspaceCreateSlug,
  WORKSPACE_CREATE_BUSY_LABEL,
  WORKSPACE_CREATE_FAILURE_TITLE,
  WORKSPACE_CREATE_INVALID_SLUG_HINT,
  WORKSPACE_CREATE_PROBE_FAILURE_MESSAGE,
  WORKSPACE_CREATE_SAVE_FAILURE_MESSAGE,
  WORKSPACE_CREATE_SIZE_REQUIRED_MESSAGE,
  WORKSPACE_CREATE_SUBMIT_LABEL,
  WORKSPACE_CREATE_SUCCESS_MESSAGE,
  WORKSPACE_CREATE_SUCCESS_TITLE,
  WORKSPACE_CREATE_TAKEN_MESSAGE,
  WORKSPACE_NAME_MAX_LENGTH,
  WORKSPACE_SLUG_MAX_LENGTH,
  WorkspaceSlugProbeError,
  WorkspaceSlugTakenError,
} from "./workspaceCreate.js";
export type { AdminThemeMode } from "./theme.js";
export { AdminAccountMenu } from "./components/AdminAccountMenu.js";
export { AI_SETTINGS_TITLE, AdminAiSettingsPage } from "./components/AdminAiSettingsPage.js";
export { AdminAuthCopyField } from "./components/AdminAuthCopyField.js";
export type { AdminAuthCopyFieldProps } from "./components/AdminAuthCopyField.js";
export { AdminAuthDiscardModal } from "./components/AdminAuthDiscardModal.js";
export type { AdminAuthDiscardModalProps } from "./components/AdminAuthDiscardModal.js";
export { AdminAuthLanding, AUTH_LANDING_TITLE } from "./components/AdminAuthLanding.js";
export { AdminAuthMethodCard } from "./components/AdminAuthMethodCard.js";
export type { AdminAuthMethodCardProps } from "./components/AdminAuthMethodCard.js";
export { AdminAuthProviderForm } from "./components/AdminAuthProviderForm.js";
export type { AdminAuthProviderFormProps, ProviderCopyHelp } from "./components/AdminAuthProviderForm.js";
export { AdminAuthProviderPage } from "./components/AdminAuthProviderPage.js";
export type { AdminAuthProviderPageProps } from "./components/AdminAuthProviderPage.js";
export { AdminAuthProviderShell } from "./components/AdminAuthProviderShell.js";
export type { AdminAuthProviderShellProps } from "./components/AdminAuthProviderShell.js";
export { AdminBrandLoader } from "./components/AdminBrandLoader.js";
export { AdminDashboardLayout } from "./components/AdminDashboardLayout.js";
export { AdminEmailPage, EMAIL_PAGE_TITLE } from "./components/AdminEmailPage.js";
export { AdminEntryGate } from "./components/AdminEntryGate.js";
export { AdminLoopJobDetailPage } from "./components/AdminLoopJobDetailPage.js";
export { AdminLoopJobModal } from "./components/AdminLoopJobModal.js";
export { AdminLoopJobsPage } from "./components/AdminLoopJobsPage.js";
export { AdminRunnersPage } from "./components/AdminRunnersPage.js";
export type { AdminEntrySearch } from "./components/AdminEntryGate.js";
export { AdminDashboardGuard, AdminHomeGuard } from "./components/AdminGuards.js";
export { AdminHeader } from "./components/AdminHeader.js";
export { AdminHelpSection } from "./components/AdminHelpSection.js";
export { ADMIN_NOT_FOUND_ART, ADMIN_NOT_FOUND_TITLE, AdminNotFoundScreen } from "./components/AdminNotFoundScreen.js";
export { AdminSidebar } from "./components/AdminSidebar.js";
export { AdminSignOutForm, SIGN_OUT_POST_PATH } from "./components/AdminSignOutForm.js";
export { FIRST_WORKSPACE_CREATE_PATH, FirstWorkspacePopup } from "./components/FirstWorkspacePopup.js";
export {
  INSTANCE_FAILURE_ART,
  INSTANCE_FAILURE_TITLE,
  InstanceFailureScreen,
  reloadEntryPage,
} from "./components/InstanceFailureScreen.js";
export { AdminGeneralSettingsPage, GENERAL_SETTINGS_TITLE } from "./components/AdminGeneralSettingsPage.js";
export { AdminSetupForm, resolveSetupPrefill, SETUP_POST_PATH, SETUP_TITLE } from "./components/AdminSetupForm.js";
export type { SetupPrefill } from "./components/AdminSetupForm.js";
export { AdminWorkspacePage } from "./components/AdminWorkspacePage.js";
export {
  AdminWorkspaceCreatePage,
  WORKSPACE_CREATE_BACK_PATH,
  WORKSPACE_CREATE_HEADING,
  WORKSPACE_CREATE_PAGE_TITLE,
} from "./components/AdminWorkspaceCreatePage.js";
export {
  WORKSPACE_CREATE_SIZE_PLACEHOLDER,
  WORKSPACE_CREATE_SUCCESS_PATH,
  WorkspaceCreateForm,
} from "./components/WorkspaceCreateForm.js";
export { WorkspaceCreationToggle } from "./components/WorkspaceCreationToggle.js";
export {
  WORKSPACE_CREATE_PATH,
  WORKSPACE_LIST_SKELETON_ROWS,
  WORKSPACE_SLUG_TOOLTIP,
  WorkspaceList,
} from "./components/WorkspaceList.js";
export { AdminSignInForm, SIGN_IN_POST_PATH, SIGN_IN_TITLE } from "./components/AdminSignInForm.js";
export { CloudAdminEntry } from "./components/CloudAdminEntry.js";
