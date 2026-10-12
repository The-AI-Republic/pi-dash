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
  EmailCheckResponse,
  MagicGenerateResponse,
  checkEmail,
  generateMagicCode,
  getCsrfToken,
  signIn,
  signInWithMagicCode,
  signOut,
} from "./contracts/auth.js";
export type { MagicSignInInput, SignInInput, SignInResult, SignOutResult } from "./contracts/auth.js";
export { Me, MeSettings, MeSettingsWorkspace, UserLite, getMe, getMeSettings } from "./contracts/users.js";
export { Workspace, WorkspaceList, WorkspaceLite, listWorkspaces } from "./contracts/workspaces.js";
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
  ProjectUserProperties,
  WorkspaceUserProperties,
  addFavoriteProject,
  archiveProject,
  createProject,
  deleteProject,
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
