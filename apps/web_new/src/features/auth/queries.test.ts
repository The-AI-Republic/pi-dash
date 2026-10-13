// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi } from "vitest";
import { authKeys, invitationQueryOptions, myInvitationsQueryOptions, myJoinRequestsQueryOptions } from "./queries.js";

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
