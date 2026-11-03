// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { SavedView, WorkspaceMember } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  findViewOwner,
  isSharedView,
  isViewLive,
  projectViewHref,
  projectViewsFeatureHref,
  projectViewsTitle,
  resolveProjectViewsBody,
  sortProjectViewsDefault,
  viewAccessLabel,
} from "./projectViews.js";

function makeView(overrides: Partial<SavedView> = {}): SavedView {
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
    owned_by: "423e4567-e89b-12d3-a456-426614174003",
    is_locked: false,
    archived_at: null,
    created_by: null,
    updated_by: null,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    ...overrides,
  };
}

function makeMember(memberId: string): WorkspaceMember {
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

describe("projectViewsTitle", () => {
  it("carries the project name followed by Views", () => {
    expect(projectViewsTitle("Website relaunch")).toBe("Website relaunch - Views");
  });
});

describe("sortProjectViewsDefault", () => {
  it("orders updated-at descending without mutating the input", () => {
    const older = makeView({ id: "223e4567-e89b-12d3-a456-426614174011", updated_at: "2026-01-01T00:00:00Z" });
    const newer = makeView({ id: "223e4567-e89b-12d3-a456-426614174012", updated_at: "2026-03-01T00:00:00Z" });
    const input = [older, newer];
    expect(sortProjectViewsDefault(input).map((view) => view.id)).toEqual([newer.id, older.id]);
    expect(input.map((view) => view.id)).toEqual([older.id, newer.id]);
  });

  it("keeps the fetched order on equal timestamps", () => {
    const first = makeView({ id: "223e4567-e89b-12d3-a456-426614174013" });
    const second = makeView({ id: "223e4567-e89b-12d3-a456-426614174014" });
    expect(sortProjectViewsDefault([first, second]).map((view) => view.id)).toEqual([first.id, second.id]);
  });
});

describe("projectViewHref", () => {
  it("links to the detail page without a trailing slash", () => {
    expect(projectViewHref("acme", "project-1", "view-9")).toBe("/acme/projects/project-1/views/view-9");
  });
});

describe("projectViewsFeatureHref", () => {
  it("targets the rendered views feature leaf, never the bare index", () => {
    const href = projectViewsFeatureHref("acme", "project-1");
    expect(href).toBe("/acme/settings/projects/project-1/features/views");
    expect(href).not.toMatch(/\/features\/?$/);
  });
});

describe("resolveProjectViewsBody", () => {
  it("shows the skeleton while the first fetch is in flight", () => {
    expect(resolveProjectViewsBody({ viewsPending: true, viewsError: false, viewCount: 0, visibleCount: 0 })).toBe(
      "loading"
    );
  });

  it("shows the error state on a read failure", () => {
    expect(resolveProjectViewsBody({ viewsPending: false, viewsError: true, viewCount: 0, visibleCount: 0 })).toBe(
      "error"
    );
  });

  it("shows the onboarding state when the project has no views, even with a search typed", () => {
    expect(resolveProjectViewsBody({ viewsPending: false, viewsError: false, viewCount: 0, visibleCount: 0 })).toBe(
      "zero"
    );
  });

  it("shows no-match when filters exclude every existing view", () => {
    expect(resolveProjectViewsBody({ viewsPending: false, viewsError: false, viewCount: 3, visibleCount: 0 })).toBe(
      "no-match"
    );
  });

  it("shows rows when any view stays visible", () => {
    expect(resolveProjectViewsBody({ viewsPending: false, viewsError: false, viewCount: 3, visibleCount: 2 })).toBe(
      "rows"
    );
  });
});

describe("view access", () => {
  it("badges shared views Public and everything else Private", () => {
    expect(isSharedView(1)).toBe(true);
    expect(isSharedView(0)).toBe(false);
    expect(viewAccessLabel(1)).toBe("Public");
    expect(viewAccessLabel(0)).toBe("Private");
  });
});

describe("isViewLive", () => {
  it("marks only anchored views", () => {
    expect(isViewLive("anchor-token")).toBe(true);
    expect(isViewLive(null)).toBe(false);
    expect(isViewLive(undefined)).toBe(false);
    expect(isViewLive("")).toBe(false);
  });
});

describe("findViewOwner", () => {
  const ownerId = "423e4567-e89b-12d3-a456-426614174003";

  it("resolves the owner against the roster", () => {
    expect(findViewOwner([makeMember(ownerId)], ownerId)).toEqual({
      name: "Ada Lovelace",
      avatarUrl: "https://cdn.example/ada.png",
    });
  });

  it("returns null when the roster is missing or the owner left", () => {
    expect(findViewOwner(undefined, ownerId)).toBeNull();
    expect(findViewOwner([], ownerId)).toBeNull();
    expect(findViewOwner([makeMember("623e4567-e89b-12d3-a456-426614174005")], ownerId)).toBeNull();
  });
});
