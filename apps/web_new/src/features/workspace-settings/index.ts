// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the workspace-settings feature (NEWFRONT-534 shell).
// Routes import the shell and layout wiring here; later slices register
// their page bodies through the registry exports.

export { WorkspaceGeneralPage } from "./general/index.js";
export { SettingsIdentity } from "./components/SettingsIdentity.js";
export type { SettingsIdentityProps } from "./components/SettingsIdentity.js";
export { SettingsMobileNav } from "./components/SettingsMobileNav.js";
export type { SettingsMobileNavProps } from "./components/SettingsMobileNav.js";
export { SettingsNavigation } from "./components/SettingsNavigation.js";
export type { SettingsNavigationProps } from "./components/SettingsNavigation.js";
export { SettingsNotAuthorized, SETTINGS_NOT_AUTHORIZED_HEADING } from "./components/SettingsNotAuthorized.js";
export type { SettingsNotAuthorizedProps } from "./components/SettingsNotAuthorized.js";
export { SettingsPageFrame } from "./components/SettingsPageFrame.js";
export type { SettingsPageFrameProps } from "./components/SettingsPageFrame.js";
export { SettingsSectionOutlet } from "./components/SettingsSectionOutlet.js";
export type { SettingsSectionOutletProps } from "./components/SettingsSectionOutlet.js";
export {
  SettingsWebhookDetailOutlet,
  WEBHOOK_DETAIL_ERROR_BODY,
  WEBHOOK_DETAIL_ERROR_HEADING,
  WEBHOOK_DETAIL_MISSING_BODY,
  WEBHOOK_DETAIL_MISSING_HEADING,
  WebhookDetailError,
  WebhookDetailMissing,
} from "./components/SettingsWebhookDetailOutlet.js";
export type { SettingsWebhookDetailOutletProps } from "./components/SettingsWebhookDetailOutlet.js";
export { SettingsShell } from "./components/SettingsShell.js";
export type { SettingsShellProps } from "./components/SettingsShell.js";
export {
  SettingsSectionPlaceholder,
  SETTINGS_SECTION_PLACEHOLDER_TITLE,
} from "./components/SettingsSectionPlaceholder.js";
export { decideWorkspaceSettingsGate } from "./lib/gate.js";
export { resolveWorkspaceSettingsIdentity } from "./lib/identity.js";
export type { WorkspaceSettingsIdentity } from "./lib/identity.js";
export type { WorkspaceSettingsGate } from "./lib/gate.js";
export { nextWorkspaceSettingsDialogOpen } from "./lib/dialog.js";
export type { WorkspaceSettingsDialogDismissReason, WorkspaceSettingsDialogOpenChange } from "./lib/dialog.js";
export {
  getWorkspaceSettingsPage,
  getWorkspaceWebhookDetailPage,
  registerWorkspaceSettingsPage,
  registerWorkspaceWebhookDetailPage,
  resetWorkspaceSettingsPages,
  useWorkspaceSettingsPage,
  useWorkspaceWebhookDetailPage,
} from "./lib/registry.js";
export type {
  WorkspaceSettingsPageComponent,
  WorkspaceSettingsPageProps,
  WorkspaceWebhookDetailPageComponent,
  WorkspaceWebhookDetailPageProps,
} from "./lib/registry.js";
export {
  canOpenWorkspaceSettingsSection,
  findWorkspaceSettingsSection,
  isWorkspaceSettingsRouteId,
  WORKSPACE_SETTINGS_ROUTE_ID,
  resolveWorkspaceSettingsSection,
  visibleWorkspaceSettingsGroups,
  WORKSPACE_SETTINGS_GROUPS,
  WORKSPACE_SETTINGS_ROOT,
  workspaceSettingsHref,
  workspaceSettingsRoleLabel,
} from "./lib/sections.js";
export type {
  VisibleWorkspaceSettingsGroup,
  VisibleWorkspaceSettingsSection,
  WorkspaceSettingsGroup,
  WorkspaceSettingsGroupKey,
  WorkspaceSettingsSection,
  WorkspaceSettingsSectionKey,
} from "./lib/sections.js";
export { BillingPage } from "./billing/BillingPage.js";
export type { BillingPageProps } from "./billing/BillingPage.js";
export { PlanCard } from "./billing/PlanCard.js";
export type { PlanCardProps } from "./billing/PlanCard.js";
export { COMPARE_COLLAPSE_LABEL, COMPARE_EXPAND_LABEL, PlanComparison } from "./billing/PlanComparison.js";
export {
  BILLING_COMPARISON,
  BILLING_PAGE_DESCRIPTION,
  BILLING_PLANS,
  COMMUNITY_PLAN,
  comparisonCellLabel,
  findBillingPlan,
  planActionHref,
  planActionLabel,
  planAmount,
  QUOTE_ON_REQUEST,
} from "./billing/model.js";
export type {
  BillingFrequency,
  ComparisonFeature,
  ComparisonSection,
  PlanDefinition,
  PlanId,
  PlanPrice,
} from "./billing/model.js";
export { WORKSPACE_SETTINGS_QUERY_POLICY } from "./lib/settingsQuery.js";
export { isWorkspaceWebhookDetailPath, workspaceSettingsTitle } from "./lib/title.js";
export type { WorkspaceSettingsTitleInput } from "./lib/title.js";
export { isMissingWebhookError, resolveWebhookDetailStatus } from "./lib/webhookRoute.js";
export {
  WebhooksListPage,
  WEBHOOKS_ADD_LABEL,
  WEBHOOKS_DESCRIPTION,
  WEBHOOKS_EMPTY_BODY,
  WEBHOOKS_EMPTY_TITLE,
  WEBHOOKS_HEADING,
  WEBHOOKS_LOAD_FAILED,
  WEBHOOKS_ROW_SWITCH_LABEL,
  webhookDetailHref,
} from "./webhooks/WebhooksListPage.js";
export type { WebhooksListPageProps, WebhookRowProps } from "./webhooks/WebhooksListPage.js";
export {
  WebhookDetailPage,
  ACTIVE_LABEL,
  DETAIL_HEADING,
  DELETE_DIALOG_BODY,
  DELETE_DIALOG_TITLE,
  DELETE_TRIGGER_LABEL,
  DANGER_ZONE_BODY,
  DANGER_ZONE_TITLE,
  REGENERATE_LABEL,
  REGENERATING_LABEL,
  SECRET_BLOCK_BODY,
  SECRET_BLOCK_TITLE,
  UPDATE_LABEL,
  UPDATING_LABEL,
  WEBHOOK_DELETED_NOTICE,
  WEBHOOK_DELETE_FAILED_NOTICE,
  WEBHOOK_REGENERATED_NOTICE,
  WEBHOOK_REGENERATE_FAILED_BODY,
  WEBHOOK_UPDATED_NOTICE,
  WEBHOOK_UPDATE_FAILED_NOTICE,
} from "./webhooks/WebhookDetailPage.js";
export type { WebhookDetailPageProps } from "./webhooks/WebhookDetailPage.js";
export {
  CreateWebhookDialog,
  CREATE_WEBHOOK_TITLE,
  KEY_CREATED_TITLE,
  KEY_CREATED_EXPLAINER,
} from "./webhooks/CreateWebhookDialog.js";
export type { CreateWebhookDialogProps } from "./webhooks/CreateWebhookDialog.js";
export { EventPicker, EVENT_MODE_LABELS } from "./webhooks/EventPicker.js";
export type { EventPickerProps, WebhookEventSelection } from "./webhooks/EventPicker.js";
export {
  SecretKey,
  SECRET_COPY_LABEL,
  SECRET_HIDE_LABEL,
  SECRET_MASK,
  SECRET_SHOW_LABEL,
} from "./webhooks/SecretKey.js";
export type { CopySecretDeps, SecretKeyProps } from "./webhooks/SecretKey.js";
export {
  ALL_EVENTS,
  eventSelectionFor,
  initialEventSelection,
  normalizeWebhookUrl,
  NO_EVENTS,
  selectMode,
  submittedFlags,
  toggleEvent,
  usesAllEvents,
  WEBHOOK_EVENT_OPTIONS,
  WEBHOOK_URL_REQUIRED,
} from "./webhooks/events.js";
export type { WebhookEventFlag, WebhookEventFlags, WebhookEventMode, WebhookEventOption } from "./webhooks/events.js";
export {
  BACKUP_COLUMNS,
  backupFileName,
  downloadTextFile,
  downloadWebhookBackup,
  webhookBackup,
  webhookBackupCsv,
} from "./webhooks/backup.js";
export type { DownloadEnvironment, WebhookBackup } from "./webhooks/backup.js";
export {
  useCreateWebhook,
  useDeleteWebhook,
  useRegenerateWebhookSecret,
  useSetWebhookActive,
  useUpdateWebhook,
  useWebhookDetail,
  useWebhooksList,
  upsertListRow,
  webhookDetailQueryOptions,
  webhookKeys,
  webhookRowOf,
  webhooksListQueryOptions,
} from "./webhooks/api.js";
export type { UpdateWebhookVariables } from "./webhooks/api.js";
export type { WorkspaceWebhookDetailStatus, WorkspaceWebhookReadState } from "./lib/webhookRoute.js";
export {
  GITLAB_PROVIDER_KEY,
  GITLAB_PROVIDER_NAME,
  GITHUB_PROVIDER_NAME,
  accountIdentity,
  canDisconnectGithub,
  canSubmitToken,
  gitProviderAccountsQueryOptions,
  githubIdentity,
  githubIntegrationQueryOptions,
  integrationsKeys,
  selectActiveGitlabAccount,
  useConnectGithub,
  useDisconnectGithub,
  useDisconnectGitlab,
  useRegisterGitlab,
} from "./integrations/integrationQueries.js";
export type { RegisterGitlabInput } from "./integrations/integrationQueries.js";
export {
  GITHUB_CONNECTED_BADGE,
  GITHUB_CONNECTED_HINT,
  GITHUB_CONNECTED_NOTICE,
  GITHUB_CONNECTED_NOTICE_BODY,
  GITHUB_CONNECTED_STATUS_PREFIX,
  GITHUB_CONNECT_FAILED_BODY,
  GITHUB_CONNECT_FAILED_NOTICE,
  GITHUB_DISCONNECTED_NOTICE,
  GITHUB_DISCONNECTED_NOTICE_BODY,
  GITHUB_DISCONNECT_FAILED_BODY,
  GITHUB_DISCONNECT_FAILED_NOTICE,
  GITHUB_EXPLAINER,
  GITHUB_HELP_HREF,
  GITHUB_HELP_LABEL,
  GITHUB_LOADING_STATUS,
  GITHUB_READ_FAILED_STATUS,
  GITHUB_RETRY_LABEL,
  GITHUB_TOKEN_LABEL,
  GITHUB_TOKEN_PLACEHOLDER,
  GithubCard,
} from "./integrations/GithubCard.js";
export type { GithubCardProps } from "./integrations/GithubCard.js";
export {
  GITLAB_CONNECTED_BADGE,
  GITLAB_CONNECTED_HINT,
  GITLAB_CONNECTED_NOTICE,
  GITLAB_CONNECTED_NOTICE_BODY,
  GITLAB_CONNECTED_STATUS_PREFIX,
  GITLAB_CONNECT_FAILED_BODY,
  GITLAB_CONNECT_FAILED_NOTICE,
  GITLAB_DISCONNECTED_NOTICE,
  GITLAB_DISCONNECTED_NOTICE_BODY,
  GITLAB_DISCONNECT_FAILED_BODY,
  GITLAB_DISCONNECT_FAILED_NOTICE,
  GITLAB_EXPLAINER,
  GITLAB_HOST_LABEL,
  GITLAB_HOST_PLACEHOLDER,
  GITLAB_LOADING_STATUS,
  GITLAB_READ_FAILED_STATUS,
  GITLAB_RETRY_LABEL,
  GITLAB_TOKEN_HINT,
  GITLAB_TOKEN_LABEL,
  GITLAB_TOKEN_PLACEHOLDER,
  GitlabCard,
  gitlabConnectedStatus,
} from "./integrations/GitlabCard.js";
export type { GitlabCardProps } from "./integrations/GitlabCard.js";
export {
  INTEGRATIONS_BANNER,
  INTEGRATIONS_HEADING,
  IntegrationsPage,
  IntegrationsScreen,
} from "./integrations/IntegrationsPage.js";
export type { IntegrationsPageProps } from "./integrations/IntegrationsPage.js";
export { ProviderCard } from "./integrations/ProviderCard.js";
export type { ProviderCardProps } from "./integrations/ProviderCard.js";
