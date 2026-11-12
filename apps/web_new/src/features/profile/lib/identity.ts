// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Identity presentation for the profile frame and sidebar
// (parity/specs/profile.md §1 and §4, PROF-001/PROF-008): the breadcrumb
// label, the full and display names, the join date, the live clock in the
// member's own timezone and the initial fallback for the avatar.

import type { UserProfileIdentity } from "@pidash/api-client";

export const OWN_PROFILE_BREADCRUMB = "Your work";

/** "First Last Work" for someone else; your own reads "Your work". */
export function profileBreadcrumbLabel(
  identity: Pick<UserProfileIdentity, "first_name" | "last_name">,
  isOwn: boolean
): string {
  if (isOwn) return OWN_PROFILE_BREADCRUMB;
  const name = `${identity.first_name} ${identity.last_name}`.trim();
  return name === "" ? "Work" : `${name} Work`;
}

/** The full name (first plus last, collapsed whitespace). */
export function profileFullName(identity: Pick<UserProfileIdentity, "first_name" | "last_name">): string {
  return `${identity.first_name} ${identity.last_name}`.replace(/\s+/g, " ").trim();
}

/** The parenthesised display name, or null when it repeats the full name's source. */
export function profileDisplayName(displayName: string): string | null {
  const trimmed = displayName.trim();
  return trimmed === "" ? null : `(${trimmed})`;
}

/** Uppercase initial of the display name for the avatar fallback. */
export function profileInitial(identity: Pick<UserProfileIdentity, "display_name" | "first_name">): string {
  const source = identity.display_name.trim() === "" ? identity.first_name : identity.display_name;
  return source.trim().charAt(0).toUpperCase();
}

/** Join date as "MMM d, yyyy" in the viewer's locale. */
export function formatJoinDate(iso: string, format: (date: Date) => string = defaultJoinFormat): string {
  return format(new Date(iso));
}

function defaultJoinFormat(date: Date): string {
  return new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric", year: "numeric" }).format(date);
}

/** Clock reading "HH:MM <timezone>" for the member's wall time. */
export function profileClock(
  timeZone: string,
  now: Date = new Date(),
  format: Intl.DateTimeFormat = new Intl.DateTimeFormat("en-US", {
    timeZone,
    hour12: false,
    hour: "2-digit",
    minute: "2-digit",
  })
): string {
  const reading = format.format(now).replace(/^24:/, "00:");
  return `${reading} ${timeZone}`;
}

/** Completion share of a member's assigned work in one project, as a percent. */
export function projectCompletion(assigned: number, completed: number): string {
  if (assigned <= 0) return "0%";
  return `${Math.round((completed / assigned) * 100)}%`;
}

/** Numeric share used for the completion tint (0–100). */
export function projectCompletionRatio(assigned: number, completed: number): number {
  if (assigned <= 0) return 0;
  return Math.min(100, Math.max(0, Math.round((completed / assigned) * 100)));
}
