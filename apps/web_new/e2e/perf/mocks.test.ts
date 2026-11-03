// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Perf mocks must satisfy the api-client contracts: the app zod-parses
// every mocked payload at runtime, so a mock that drifts from its schema
// fails the perf specs in CI minutes later instead of failing here in
// milliseconds (NEWFRONT-585: the sharpened project detail broke the
// issues-list specs until the mock carried the executor fields).
import { describe, expect, it } from "vitest";
import {
  InstanceBootstrap,
  IssueListResponse,
  Me,
  OwnWorkspaceMembership,
  Profile,
  ProjectDetail,
  ProjectList,
  ProjectRoleMap,
  ProjectUserProperties,
  SidebarPreferences,
  UnreadNotifications,
  WorkspaceList,
} from "@pidash/api-client";
import {
  mockInstanceBootstrap,
  mockIssue,
  mockIssues,
  mockMe,
  mockMembership,
  mockProfile,
  mockProjectDetail,
  mockProjectRoles,
  mockProjectUserProperties,
  mockProjects,
  mockSidebarPreferences,
  mockUnreadNotifications,
  mockWorkspaces,
} from "./mocks.js";

describe("perf mocks satisfy their contracts", () => {
  it("parses every mocked payload", () => {
    expect(() => InstanceBootstrap.parse(mockInstanceBootstrap())).not.toThrow();
    expect(mockInstanceBootstrap().instance.is_setup_done).toBe(true);
    expect(() => Me.parse(mockMe())).not.toThrow();
    expect(() => Profile.parse(mockProfile())).not.toThrow();
    expect(() => WorkspaceList.parse(mockWorkspaces())).not.toThrow();
    expect(() => ProjectList.parse(mockProjects())).not.toThrow();
    expect(() => ProjectDetail.parse(mockProjectDetail())).not.toThrow();
    expect(() => OwnWorkspaceMembership.parse(mockMembership())).not.toThrow();
    expect(() => ProjectRoleMap.parse(mockProjectRoles())).not.toThrow();
    expect(() => UnreadNotifications.parse(mockUnreadNotifications())).not.toThrow();
    expect(() => SidebarPreferences.parse(mockSidebarPreferences())).not.toThrow();
    expect(() => ProjectUserProperties.parse(mockProjectUserProperties())).not.toThrow();
    expect(() => IssueListResponse.parse(mockIssues(3))).not.toThrow();
    expect(mockIssue(0).project_id).toBe(mockProjects()[0]?.id);
  });
});
