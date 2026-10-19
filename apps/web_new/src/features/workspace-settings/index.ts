// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the workspace-settings feature (NEWFRONT-534 shell).
// Routes import the shell and layout wiring here; later slices register
// their page bodies through the registry exports.

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
export type { WorkspaceWebhookDetailStatus, WorkspaceWebhookReadState } from "./lib/webhookRoute.js";
