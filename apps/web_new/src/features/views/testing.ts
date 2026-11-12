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

export function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) } as TransportResponse;
}
