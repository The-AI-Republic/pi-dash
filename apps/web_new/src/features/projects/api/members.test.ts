// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi } from "vitest";

vi.mock("@pidash/api-client", () => ({
  listWorkspaceMembers: vi.fn(),
}));

import { listWorkspaceMembers } from "@pidash/api-client";

import { workspaceMembersQueryOptions } from "./members.js";

const CLIENT = { name: "stub-client" } as never;

describe("projectKeys.members", () => {
  it("scopes memberships under the workspace projects subtree", async () => {
    vi.mocked(listWorkspaceMembers).mockResolvedValue([]);
    const options = workspaceMembersQueryOptions(CLIENT, "acme");
    expect(options.queryKey).toEqual(["ws", "acme", "projects", "members"]);
    if (!options.queryFn) throw new Error("missing queryFn");
    await options.queryFn({} as never);
    expect(listWorkspaceMembers).toHaveBeenCalledWith(CLIENT, "acme");
  });
});
