// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Creator display for the detail sidebar (parity/specs/issues.md §13,
// ISS-152). The creator joins to the workspace directory for a name and
// avatar. Items filed by the intake bot present under the product name
// with no person attached, in the detail and the peek alike.

import type { WorkspaceMemberList } from "@pidash/api-client";

/** Product name that masks intake-bot creators. */
export const PRODUCT_CREATOR_NAME = "Pi Dash";

/** Fallback when the creator is no longer in the directory. */
export const UNKNOWN_CREATOR_NAME = "Unknown member";

export interface CreatorDisplay {
  name: string;
  avatarUrl: string | null;
  /** True when the row stands in for the intake bot rather than a person. */
  masked: boolean;
}

/** Creator row for an issue's created_by; null when nobody is recorded. */
export function selectCreator(members: WorkspaceMemberList | undefined, userId: string | null): CreatorDisplay | null {
  if (userId === null) return null;
  const entry = members?.find((row) => row.member.id === userId);
  if (entry?.member.is_bot === true) {
    return { name: PRODUCT_CREATOR_NAME, avatarUrl: null, masked: true };
  }
  if (entry === undefined) {
    return { name: UNKNOWN_CREATOR_NAME, avatarUrl: null, masked: false };
  }
  return { name: entry.member.display_name, avatarUrl: entry.member.avatar_url ?? null, masked: false };
}
