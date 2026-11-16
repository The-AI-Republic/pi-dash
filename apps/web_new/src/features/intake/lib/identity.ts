// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The intake system identity (parity/specs/intake.md INT-006). Rows the
// system authored — forms or email ingestion — show an intake avatar
// instead of a member avatar. The rule is about the creating identity,
// never about the row's `source`: a member-created forms row keeps its
// member avatar. Where the identity is read from is an open question
// (Decisions Q10); until a server field exists the predicate compares
// against this one named address, which the parity seed's intake user
// carries.
import type { WorkspaceMemberList } from "@pidash/api-client";

/**
 * The intake system identity's address. The parity fixtures create the
 * intake user at this address, so rows that user authors read as
 * system-authored.
 */
export const INTAKE_IDENTITY_EMAIL = "intake@example.com";

/**
 * Whether `creatorId` is the intake identity: the directory entry
 * behind it carries the intake address. A null creator, a creator the
 * directory does not know, or a lens without emails reads as a member
 * row, never as the identity.
 */
export function isIntakeIdentityCreator(directory: WorkspaceMemberList | undefined, creatorId: string | null): boolean {
  if (creatorId === null || directory === undefined) return false;
  const entry = directory.find((row) => row.member.id === creatorId);
  return entry?.member.email === INTAKE_IDENTITY_EMAIL;
}
