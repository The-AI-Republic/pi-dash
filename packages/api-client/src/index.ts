// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// @pidash/api-client: fetch client, endpoint contracts, contract-test harness.
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
export {
  CsrfTokenResponse,
  DeviceApproveResponse,
  EmailCheckResponse,
  ForgotPasswordResponse,
  InstanceConfig,
  MagicGenerateResponse,
  SetPasswordResult,
  approveDeviceCode,
  checkEmail,
  deactivateAccount,
  generateMagicCode,
  getCsrfToken,
  getInstanceConfig,
  parseAuthRedirectResult,
  requestPasswordReset,
  resetPassword,
  setPassword,
  signIn,
  signInWithMagicCode,
  signOut,
  signUp,
  signUpWithMagicCode,
} from "./contracts/auth.js";
export type {
  MagicSignInInput,
  MagicSignUpInput,
  ResetPasswordInput,
  SignInInput,
  SignInResult,
  SignOutResult,
  SignUpInput,
} from "./contracts/auth.js";
export {
  Me,
  MeSettings,
  MeSettingsWorkspace,
  MeUpdate,
  UserLite,
  getMe,
  getMeSettings,
  markOnboarded,
  updateMe,
} from "./contracts/users.js";
export type { MePatch } from "./contracts/users.js";
export {
  CreatedWorkspace,
  OwnWorkspaceMembership,
  Workspace,
  WorkspaceList,
  WorkspaceLite,
  WorkspaceSlugStatus,
  checkWorkspaceSlug,
  createWorkspace,
  getMyWorkspaceMembership,
  listWorkspaces,
} from "./contracts/workspaces.js";
export type { WorkspaceCreateInput } from "./contracts/workspaces.js";
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
  FavoriteProject,
  FavoriteProjectList,
  InvitationProject,
  InvitationWorkspace,
  JoinProjectsResult,
  ProjectArchiveResult,
  ProjectDetails,
  ProjectDetailsList,
  ProjectInvitation,
  ProjectInvitationList,
  ProjectRoleMap,
  ProjectUserProperties,
  WorkspaceUserProperties,
  addFavoriteProject,
  archiveProject,
  createProject,
  deleteProject,
  getMyProjectRoles,
  getProjectUserProperties,
  getWorkspaceUserProperties,
  joinProjects,
  leaveProject,
  listFavoriteProjects,
  listProjectDetails,
  listProjectInvitations,
  removeFavoriteProject,
  unarchiveProject,
  updateProjectUserProperties,
  updateWorkspaceUserProperties,
} from "./contracts/projects.js";
export type { ProjectCreateInput } from "./contracts/projects.js";
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
  IssueSubscription,
  SubscriptionStatus,
  getIssue,
  getSubscriptionStatus,
  listIssues,
  subscribeIssue,
  unsubscribeIssue,
  updateIssue,
} from "./contracts/issues.js";
export type { IssueListQuery, IssuePatch } from "./contracts/issues.js";
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
export {
  MeTimezone,
  Profile,
  TimezoneEntry,
  TimezoneList,
  getProfile,
  listTimezones,
  updateMyTimezone,
  updateProfile,
} from "./contracts/profile.js";
export type { ProfilePatch } from "./contracts/profile.js";
export {
  AssistantThread,
  AssistantThreadList,
  LlmConfig,
  createAssistantThread,
  getLlmConfig,
  listAssistantThreads,
} from "./contracts/assistant.js";
export { UnreadNotifications, getUnreadNotifications } from "./contracts/notifications.js";
export { CycleUpdate, updateCycle } from "./contracts/cycles.js";
export type { CyclePatch } from "./contracts/cycles.js";
export { ModuleUpdate, updateModule } from "./contracts/modules.js";
export type { ModulePatch } from "./contracts/modules.js";
export { PageUpdate, updatePage } from "./contracts/pages.js";
export type { PagePatch } from "./contracts/pages.js";
