// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Views test fixtures. Tests only; never imported by production code.

import type { SavedView, TransportResponse, WorkspaceMember } from "@pidash/api-client";

export const VIEWS_OWNER_ID = "423e4567-e89b-12d3-a456-426614174003";

export function makeView(overrides: Partial<SavedView> = {}): SavedView {
  return {
    id: "223e4567-e89b-12d3-a456-426614174001",
    workspace: "123e4567-e89b-12d3-a456-426614174000",
    project: "323e4567-e89b-12d3-a456-426614174002",
    name: "Triage",
    description: "",
    query: {},
    filters: {},
    display_filters: {},
    display_properties: {},
    rich_filters: {},
    access: 1,
    sort_order: 0,
    logo_props: {},
    owned_by: VIEWS_OWNER_ID,
    is_locked: false,
    archived_at: null,
    created_by: null,
    updated_by: null,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    ...overrides,
  };
}

export function makeMember(memberId: string = VIEWS_OWNER_ID): WorkspaceMember {
  return {
    id: "523e4567-e89b-12d3-a456-426614174004",
    member: {
      id: memberId,
      first_name: "Ada",
      last_name: "Lovelace",
      avatar: "",
      avatar_url: "https://cdn.example/ada.png",
      display_name: "Ada Lovelace",
      is_bot: false,
    },
    role: 15,
  };
}

export function makeMemberWithName(memberId: string, displayName: string): WorkspaceMember {
  const base = makeMember(memberId);
  return {
    ...base,
    member: {
      ...base.member,
      display_name: displayName,
      first_name: displayName.split(" ")[0] ?? displayName,
      last_name: displayName.split(" ").slice(1).join(" ") ?? "",
    },
  };
}

export interface ProjectMemberRow {
  id: string;
  role: number;
  member: string;
  project: string;
  original_role: number;
  created_at: string;
}

let __pmCounter = 0;

export function makeProjectMember(
  userId: string,
  projectId: string = "223e4567-e89b-12d3-a456-426614174001",
  overrides: Partial<ProjectMemberRow> = {}
): ProjectMemberRow {
  // Valid UUID v4-shaped row for each membership; the filter key is `member`.
  __pmCounter += 1;
  const id = `10000000-0000-4000-8000-${String(__pmCounter).padStart(12, "0")}`;
  return {
    id,
    role: 15,
    member: userId,
    project: projectId,
    original_role: 15,
    created_at: "2026-01-01T00:00:00.000Z",
    ...overrides,
  };
}

export function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) } as TransportResponse;
}
