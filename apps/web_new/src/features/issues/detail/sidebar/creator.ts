// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Creator display for the detail sidebar (parity/specs/issues.md §13,
// ISS-152). The creator joins to the workspace directory for a name and
// avatar. Items filed by the intake bot present under the product name
// with no person attached; every other member, bot or not, keeps its
// own name.

import type { WorkspaceMemberList } from "@pidash/api-client";

/** Product name that masks intake-bot creators. */
export const PRODUCT_CREATOR_NAME = "Pi Dash";

/** Fallback when the creator is no longer in the directory. */
export const UNKNOWN_CREATOR_NAME = "Unknown member";

/** Suffix the intake bot carries in its display name. */
const INTAKE_NAME_MARKER = "-intake";

export interface CreatorDisplay {
  name: string;
  avatarUrl: string | null;
  /** True when the row stands in for the intake bot rather than a person. */
  masked: boolean;
}

/**
 * True for the intake bot, the only creator hidden behind the product
 * name. The directory read is the only source: the bot announces itself
 * in its display name, while other bots (seed, agent users) are named
 * as themselves.
 */
export function isIntakeBot(member: { display_name: string }): boolean {
  return member.display_name.toLowerCase().includes(INTAKE_NAME_MARKER);
}

/** Creator row for an issue's created_by; null when nobody is recorded. */
export function selectCreator(members: WorkspaceMemberList | undefined, userId: string | null): CreatorDisplay | null {
  if (userId === null) return null;
  const entry = members?.find((row) => row.member.id === userId);
  if (entry === undefined) {
    return { name: UNKNOWN_CREATOR_NAME, avatarUrl: null, masked: false };
  }
  if (isIntakeBot(entry.member)) {
    return { name: PRODUCT_CREATOR_NAME, avatarUrl: null, masked: true };
  }
  return { name: entry.member.display_name, avatarUrl: entry.member.avatar_url ?? null, masked: false };
}
