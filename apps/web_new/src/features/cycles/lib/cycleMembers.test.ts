// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Member directory: user ids resolve to names and avatars; unknown ids
// keep a stable fallback.

import { describe, expect, it } from "vitest";

import type { WorkspaceMemberList } from "@pidash/api-client";

import { selectCycleMemberDirectory, UNKNOWN_CYCLE_MEMBER } from "./cycleMembers.js";

const MEMBER_ID = "123e4567-e89b-12d3-a456-426614174010";

function directory(): WorkspaceMemberList {
  return [
    {
      member: { id: MEMBER_ID, display_name: "Ada Lovelace", avatar_url: "https://cdn.test/ada.png" },
    },
  ] as WorkspaceMemberList;
}

describe("cycle member directory", () => {
  it("keys display names and avatars by user id", () => {
    const members = selectCycleMemberDirectory(directory());
    expect(members.get(MEMBER_ID)).toEqual({ displayName: "Ada Lovelace", avatarUrl: "https://cdn.test/ada.png" });
    expect(members.get("missing")).toBeUndefined();
  });

  it("falls back to a stable unknown member", () => {
    expect(UNKNOWN_CYCLE_MEMBER).toEqual({ displayName: "Unknown member", avatarUrl: null });
  });
});
