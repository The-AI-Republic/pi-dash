// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// @pidash/api-client: fetch client, endpoint contracts, contract-test harness.
export { SignedUserAsset, uploadUserAvatar } from "./contracts/assets.js";
export type { ByteUpload, ByteUploadResponse } from "./contracts/assets.js";
export { ApiError, isApiError, isRetryable } from "./errors.js";
export type { ApiErrorCode, ApiErrorFields, ApiErrorInit } from "./errors.js";
export { ApiClient, createClient, CSRF_ENDPOINT, CSRF_HEADER } from "./client.js";
export type {
  ClientOptions,
  HttpMethod,
  Middleware,
  MiddlewareContext,
  ParsedEnvelope,
  RequestOptions,
  Transport,
  TransportInit,
  TransportResponse,
} from "./client.js";
export { InstanceConfig, deactivateAccount, getInstanceConfig, signOut } from "./contracts/auth.js";
export type { SignOutResult } from "./contracts/auth.js";
export {
  CsrfTokenResponse,
  DeviceApproveResponse,
  EmailCheckResponse,
  ForgotPasswordResponse,
  InstanceBootstrap,
  InstanceDetails,
  InstanceInfo,
  MagicGenerateResponse,
  SetPasswordResult,
  approveDeviceCode,
  checkEmail,
  generateMagicCode,
  getCsrfToken,
  getInstanceBootstrap,
  parseAuthRedirectResult,
  requestPasswordReset,
  resetPassword,
  setPassword,
  signIn,
  signInWithMagicCode,
  signUp,
  signUpWithMagicCode,
} from "./contracts/auth-entry.js";
export type {
  MagicSignInInput,
  MagicSignUpInput,
  ResetPasswordInput,
  SignInInput,
  SignInResult,
  SignUpInput,
} from "./contracts/auth-entry.js";
export { Me, MeSettings, MeSettingsWorkspace, UserLite, getMe, getMeSettings } from "./contracts/users.js";
export { MeUpdate, markOnboarded, updateMe } from "./contracts/user-writes.js";
export type { MePatch } from "./contracts/user-writes.js";
export {
  OwnWorkspaceMembership,
  Workspace,
  WorkspaceList,
  WorkspaceLite,
  getMyWorkspaceMembership,
  listWorkspaces,
} from "./contracts/workspaces.js";
export {
  CreatedWorkspace,
  WorkspaceMember,
  WorkspaceMemberList,
  WorkspaceSlugStatus,
  checkWorkspaceSlug,
  createWorkspace,
  listWorkspaceMembers,
} from "./contracts/workspace-management.js";
export type { WorkspaceCreateInput } from "./contracts/workspace-management.js";
export {
  BulkInviteResult,
  InvitationAnswer,
  JoinRequestRequester,
  JoinRequestResult,
  WorkspaceInvitation,
  WorkspaceInvitationList,
  WorkspaceJoinRequest,
  WorkspaceJoinRequestList,
  acceptWorkspaceInvitations,
  answerWorkspaceInvitation,
  createWorkspaceJoinRequest,
  getWorkspaceInvitation,
  listWorkspaceInvitations,
  listWorkspaceJoinRequests,
  sendWorkspaceInvitations,
} from "./contracts/invitations.js";
export type {
  InvitationAnswerInput,
  JoinRequestInput,
  WorkspaceInviteInput,
  WorkspaceInvitee,
} from "./contracts/invitations.js";
export { Project, ProjectList, ProjectLite, listProjects } from "./contracts/projects.js";
export {
  ProjectDetail,
  ProjectDetails,
  ProjectDetailsList,
  ProjectRoleMap,
  ProjectUserProperties,
  WorkspaceUserProperties,
  getMyProjectRoles,
  getProject,
  getProjectUserProperties,
  getWorkspaceUserProperties,
  listProjectDetails,
} from "./contracts/projects.js";
export {
  FavoriteProject,
  FavoriteProjectList,
  InvitationProject,
  InvitationWorkspace,
  JoinProjectsResult,
  ProjectArchiveResult,
  ProjectIdentifierCheck,
  ProjectIdentifierMatch,
  ProjectInvitation,
  ProjectInvitationList,
  addFavoriteProject,
  archiveProject,
  checkProjectIdentifier,
  createProject,
  deleteProject,
  joinProjects,
  leaveProject,
  listFavoriteProjects,
  listProjectInvitations,
  removeFavoriteProject,
  unarchiveProject,
  updateProject,
  updateProjectUserProperties,
  updateWorkspaceUserProperties,
} from "./contracts/project-management.js";
export type { ProjectCreateInput, ProjectUpdateInput } from "./contracts/project-management.js";
export {
  DashboardActivityBucket,
  DashboardCompletedBucket,
  DashboardOverdueIssue,
  DashboardStateSlice,
  DashboardUpcomingIssue,
  HOME_WIDGET_KEYS,
  HomePreference,
  HomePreferenceList,
  HomePreferenceUpdate,
  QuickLink,
  QuickLinkList,
  RecentVisit,
  RecentVisitList,
  TourCompletedResponse,
  WorkspaceDashboard,
  completeTour,
  createQuickLink,
  deleteQuickLink,
  getQuickLink,
  getWorkspaceDashboard,
  listHomePreferences,
  listQuickLinks,
  listRecentVisits,
  updateHomePreference,
  updateQuickLink,
} from "./contracts/home.js";
export type { HomePreferencePatch, HomeWidgetKey, QuickLinkInput } from "./contracts/home.js";
export {
  Label,
  LabelList,
  ProjectMember,
  ProjectMemberList,
  STATE_GROUPS,
  State,
  StateList,
  listLabels,
  listMembers,
  listStates,
} from "./contracts/reference.js";
export {
  ISSUE_PRIORITIES,
  Issue,
  IssueListItem,
  IssueListResponse,
  IssueRelationsSummary,
  getIssue,
  listArchivedIssues,
  listIssues,
} from "./contracts/issues.js";
export type { IssueListQuery } from "./contracts/issues.js";
export {
  IssueCreateResult,
  IssueSubscription,
  MembershipWriteResult,
  SubscriptionStatus,
  addIssueToCycle,
  createIssue,
  deleteIssue,
  getSubscriptionStatus,
  removeIssueFromCycle,
  setIssueModules,
  subscribeIssue,
  unsubscribeIssue,
  updateIssue,
} from "./contracts/issue-actions.js";
export type { IssueCreateInput, IssuePatch } from "./contracts/issue-actions.js";
export {
  Favorite,
  FavoriteList,
  SidebarPreferences,
  SidebarPreferencesPatchResult,
  SidebarPreferenceItem,
  createFavorite,
  deleteFavorite,
  getSidebarPreferences,
  listFavoriteGroup,
  listFavorites,
  updateFavorite,
  updateSidebarPreferences,
} from "./contracts/sidebar.js";
export type { FavoriteCreate, FavoritePatch, SidebarPreferencePatch } from "./contracts/sidebar.js";
export { SEARCH_ENTITIES, SearchResults, searchWorkspace } from "./contracts/search.js";
export type { SearchEntity, WorkspaceSearchQuery } from "./contracts/search.js";
export { Profile, getProfile, updateProfile } from "./contracts/profile.js";
export type { ProfilePatch } from "./contracts/profile.js";
export { MeTimezone, TimezoneEntry, TimezoneList, listTimezones, updateMyTimezone } from "./contracts/locale.js";
export {
  AssistantThread,
  AssistantThreadList,
  LlmConfig,
  createAssistantThread,
  getLlmConfig,
  listAssistantThreads,
} from "./contracts/assistant.js";
export {
  MarkAllReadResponse,
  NotificationItem,
  NotificationList,
  NotificationListResponse,
  NotificationPreferences,
  UnreadNotifications,
  archiveNotification,
  getNotificationPreferences,
  getUnreadNotifications,
  listNotifications,
  listNotificationsPage,
  markAllNotificationsRead,
  markNotificationRead,
  markNotificationUnread,
  snoozeNotification,
  unarchiveNotification,
  updateNotificationPreferences,
} from "./contracts/notifications.js";
export type {
  MarkAllReadScope,
  NotificationListQuery,
  NotificationPageQuery,
  NotificationPreferencePatch,
} from "./contracts/notifications.js";
export { CycleList, CycleListItem, CycleUpdate, listCycles, updateCycle } from "./contracts/cycles.js";
export type { CyclePatch } from "./contracts/cycles.js";
export {
  MODULE_STATUSES,
  ModuleList,
  ModuleListItem,
  ModuleUpdate,
  listModules,
  updateModule,
} from "./contracts/modules.js";
export type { ModulePatch, ModuleStatus } from "./contracts/modules.js";
export {
  PAGE_ACCESS_PRIVATE,
  PAGE_ACCESS_PUBLIC,
  PageArchiveResult,
  PageUpdate,
  addPageFavorite,
  archivePage,
  lockPage,
  removePageFavorite,
  restorePage,
  setPageAccess,
  unlockPage,
  updatePage,
} from "./contracts/pages.js";
export type { PagePatch } from "./contracts/pages.js";
export {
  ESTIMATE_SYSTEM_TIME,
  Estimate,
  EstimateList,
  EstimatePoint,
  listProjectEstimates,
  selectActiveEstimate,
} from "./contracts/estimates.js";
export {
  AdminWorkspace,
  AdminWorkspaceListResponse,
  AdminWorkspaceOwner,
  InstanceWorkspaceSlugStatus,
  checkInstanceWorkspaceSlug,
  createInstanceWorkspace,
  listInstanceWorkspaces,
} from "./contracts/admin-workspaces.js";
export type { InstanceWorkspaceCreateInput } from "./contracts/admin-workspaces.js";
export {
  LOOP_MIN_ROLES,
  LOOP_SKIP_REASONS,
  LoopJob,
  LoopJobDetail,
  LoopJobList,
  LoopJobStats,
  LoopTarget,
  LoopTargetLastRun,
  LoopTargetsPage,
  createLoopJob,
  deleteLoopJob,
  getLoopJob,
  listLoopJobs,
  listLoopTargets,
  updateLoopJob,
} from "./contracts/admin-loop.js";
export type {
  LoopJobCreateInput,
  LoopJobPatchInput,
  LoopMinRole,
  LoopSkipReason,
  LoopTargetsQuery,
} from "./contracts/admin-loop.js";
export {
  AdminMe,
  CloudAuthUser,
  EmailCredentialCheckResult,
  InstanceAdminEntry,
  InstanceAdminList,
  InstanceAdminUser,
  InstanceConfiguration,
  InstanceConfigurationList,
  adminSignIn,
  adminSignOut,
  adminSignUp,
  checkEmailCredentials,
  cloudSignInUrl,
  disableEmailFeature,
  getAdminSession,
  listInstanceAdmins,
  listInstanceConfigurations,
  probeCloudAuthSession,
  updateInstance,
  updateInstanceConfigurations,
} from "./contracts/instance.js";
export type {
  AdminSession,
  AdminSignInInput,
  AdminSignUpInput,
  CloudAuthProbe,
  InstancePatch,
} from "./contracts/instance.js";
