// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi } from "vitest";
import { invitationQueryOptions, myInvitationsQueryOptions, myJoinRequestsQueryOptions } from "./invitations.js";
import { authKeys, membershipQueryOptions, projectQueryOptions, projectRolesQueryOptions } from "./queries.js";

describe("invitation query", () => {
  it("keys one invitation by slug and id", () => {
    expect(authKeys.invitation("acme", "invite-1")).toEqual(["auth", "invitation", "acme", "invite-1"]);
  });

  it("fetches the emailed invitation through the join URL", async () => {
    const invitation = { id: "invite-1", email: "ada@example.com" };
    const getJson = vi.fn(async () => invitation);
    const fakeClient = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
    const options = invitationQueryOptions(fakeClient, "acme", "invite-1");
    expect(options.queryKey).toEqual(authKeys.invitation("acme", "invite-1"));
    expect(options.enabled).toBe(true);
    await expect(options.queryFn?.({} as never)).resolves.toEqual(invitation);
    expect(getJson).toHaveBeenCalledWith("/api/workspaces/acme/invitations/invite-1/join/");
  });

  it("stays disabled without both invitation params", () => {
    const fakeClient = {
      getJson: () => Promise.resolve({}),
      parse: (_schema: unknown, data: unknown) => data,
    } as never;
    expect(invitationQueryOptions(fakeClient, "", "invite-1").enabled).toBe(false);
    expect(invitationQueryOptions(fakeClient, "acme", "").enabled).toBe(false);
    expect(invitationQueryOptions(fakeClient, "", "").enabled).toBe(false);
  });
});

describe("own invitations and join requests", () => {
  it("keys the caller's invitation list", async () => {
    const invitations = [{ id: "invite-1" }];
    const getJson = vi.fn(async () => invitations);
    const fakeClient = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
    const options = myInvitationsQueryOptions(fakeClient);
    expect(options.queryKey).toEqual(authKeys.myInvitations);
    await expect(options.queryFn?.({} as never)).resolves.toEqual(invitations);
    expect(getJson).toHaveBeenCalledWith("/api/users/me/workspaces/invitations/");
  });

  it("keys the caller's join-request list", async () => {
    const requests = [{ id: "request-1" }];
    const getJson = vi.fn(async () => requests);
    const fakeClient = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
    const options = myJoinRequestsQueryOptions(fakeClient);
    expect(options.queryKey).toEqual(authKeys.myJoinRequests);
    await expect(options.queryFn?.({} as never)).resolves.toEqual(requests);
    expect(getJson).toHaveBeenCalledWith("/api/users/me/workspaces/join-requests/");
  });
});

describe("denial feed queries", () => {
  function fakeClient(body: unknown) {
    const getJson = vi.fn(async () => body);
    const client = {
      getJson,
      parse: (_schema: unknown, data: unknown) => data,
    } as never;
    return { client, getJson };
  }

  it("keys the feeds under the workspace scope", () => {
    expect(authKeys.membership("acme")).toEqual(["ws", "acme", "auth", "membership"]);
    expect(authKeys.projectRoles("acme")).toEqual(["ws", "acme", "auth", "project-roles"]);
    expect(authKeys.project("acme", "project-1")).toEqual(["ws", "acme", "auth", "project", "project-1"]);
  });

  it("reads the caller's own membership", async () => {
    const row = { member: "user-1", role: 20 };
    const { client, getJson } = fakeClient(row);
    const options = membershipQueryOptions(client, "acme");
    expect(options.queryKey).toEqual(authKeys.membership("acme"));
    await expect(options.queryFn?.({} as never)).resolves.toEqual(row);
    expect(getJson).toHaveBeenCalledWith("/api/workspaces/acme/workspace-members/me/");
  });

  it("reads the caller's per-project roles", async () => {
    const roles = { "project-1": 20 };
    const { client, getJson } = fakeClient(roles);
    const options = projectRolesQueryOptions(client, "acme");
    expect(options.queryKey).toEqual(authKeys.projectRoles("acme"));
    await expect(options.queryFn?.({} as never)).resolves.toEqual(roles);
    expect(getJson).toHaveBeenCalledWith("/api/users/me/workspaces/acme/project-roles/");
  });

  it("reads one project row for its fetch status", async () => {
    const row = { id: "project-1", name: "Web" };
    const { client, getJson } = fakeClient(row);
    const options = projectQueryOptions(client, "acme", "project-1");
    expect(options.queryKey).toEqual(authKeys.project("acme", "project-1"));
    await expect(options.queryFn?.({} as never)).resolves.toEqual(row);
    expect(getJson).toHaveBeenCalledWith("/api/workspaces/acme/projects/project-1/");
  });
});
