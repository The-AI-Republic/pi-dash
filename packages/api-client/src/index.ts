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
  listIssues,
} from "./contracts/issues.js";
export type { IssueListQuery } from "./contracts/issues.js";
