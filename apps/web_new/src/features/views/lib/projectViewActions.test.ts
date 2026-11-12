// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { Favorite } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { makeView } from "../testing.js";
import {
  canDeleteProjectView,
  canEditProjectView,
  canStarProjectView,
  findViewFavoriteId,
  flipViewFavorite,
  isViewFavorited,
  removeProjectViewRow,
  replaceProjectViewRow,
  selectProjectViewMenuItems,
  setProjectViewAnchor,
  withViewAnchor,
} from "./projectViewActions.js";

const OWNER = "423e4567-e89b-12d3-a456-426614174003";
const MEMBER = "623e4567-e89b-12d3-a456-426614174005";
const VIEW_ID = "223e4567-e89b-12d3-a456-426614174001";
const OTHER_ID = "323e4567-e89b-12d3-a456-426614174002";

describe("project view action gates", () => {
  it("stars for members and admins, never for guests or outsiders", () => {
    expect(canStarProjectView(20)).toBe(true);
    expect(canStarProjectView(15)).toBe(true);
    expect(canStarProjectView(5)).toBe(false);
    expect(canStarProjectView(null)).toBe(false);
  });

  it("edits for the owner only", () => {
    const view = makeView({ owned_by: OWNER });
    expect(canEditProjectView(OWNER, view)).toBe(true);
    expect(canEditProjectView(MEMBER, view)).toBe(false);
    expect(canEditProjectView(null, view)).toBe(false);
  });

  it("deletes for the owner or a project admin", () => {
    const view = makeView({ owned_by: OWNER });
    expect(canDeleteProjectView(OWNER, view, 15)).toBe(true);
    expect(canDeleteProjectView(MEMBER, view, 20)).toBe(true);
    expect(canDeleteProjectView(MEMBER, view, 15)).toBe(false);
    expect(canDeleteProjectView(MEMBER, view, 5)).toBe(false);
    expect(canDeleteProjectView(null, view, 20)).toBe(true);
    expect(canDeleteProjectView(null, view, null)).toBe(false);
  });

  it("orders menu entries per standing", () => {
    expect(selectProjectViewMenuItems({ isOwner: true, isAdmin: false, canPublish: false })).toEqual([
      "edit",
      "open",
      "copy",
      "delete",
    ]);
    expect(selectProjectViewMenuItems({ isOwner: false, isAdmin: false, canPublish: false })).toEqual(["open", "copy"]);
    expect(selectProjectViewMenuItems({ isOwner: false, isAdmin: true, canPublish: false })).toEqual([
      "open",
      "copy",
      "delete",
    ]);
  });

  it("seats publish after copy for the owner or an admin on cloud", () => {
    expect(selectProjectViewMenuItems({ isOwner: true, isAdmin: false, canPublish: true })).toEqual([
      "edit",
      "open",
      "copy",
      "publish",
      "delete",
    ]);
    expect(selectProjectViewMenuItems({ isOwner: false, isAdmin: true, canPublish: true })).toEqual([
      "open",
      "copy",
      "publish",
      "delete",
    ]);
    expect(selectProjectViewMenuItems({ isOwner: false, isAdmin: false, canPublish: true })).toEqual(["open", "copy"]);
  });
});

describe("project view list-cache patches", () => {
  it("reads the star as selected only when true", () => {
    expect(isViewFavorited(true)).toBe(true);
    expect(isViewFavorited(false)).toBe(false);
    expect(isViewFavorited(null)).toBe(false);
    expect(isViewFavorited(undefined)).toBe(false);
  });

  it("flips one row's star and leaves the rest", () => {
    const list = [makeView({ id: VIEW_ID, is_favorite: false }), makeView({ id: OTHER_ID, is_favorite: true })];
    expect(flipViewFavorite(list, VIEW_ID)?.map((row) => row.is_favorite)).toEqual([true, true]);
    expect(flipViewFavorite(list, OTHER_ID)?.map((row) => row.is_favorite)).toEqual([false, false]);
    expect(flipViewFavorite(list, "nope")?.map((row) => row.is_favorite)).toEqual([false, true]);
  });

  it("replaces one row with its echo and leaves the rest", () => {
    const list = [makeView({ id: VIEW_ID, name: "before" }), makeView({ id: OTHER_ID })];
    const echo = makeView({ id: VIEW_ID, name: "after" });
    const next = replaceProjectViewRow(list, echo);
    expect(next?.map((row) => row.name)).toEqual(["after", "Triage"]);
    expect(next?.[1]).toBe(list[1]);
    expect(replaceProjectViewRow(list, makeView({ id: "nope" }))?.map((row) => row.name)).toEqual(["before", "Triage"]);
  });

  it("removes the deleted row and leaves the rest", () => {
    const list = [makeView({ id: VIEW_ID }), makeView({ id: OTHER_ID })];
    expect(removeProjectViewRow(list, VIEW_ID)?.map((row) => row.id)).toEqual([OTHER_ID]);
    expect(removeProjectViewRow(list, "nope")?.map((row) => row.id)).toEqual([VIEW_ID, OTHER_ID]);
  });

  it("sets or clears one row's anchor and leaves the rest", () => {
    const list = [makeView({ id: VIEW_ID }), makeView({ id: OTHER_ID })];
    expect(setProjectViewAnchor(list, VIEW_ID, "abc")?.map((row) => row.anchor)).toEqual(["abc", undefined]);
    expect(setProjectViewAnchor(list, VIEW_ID, null)?.map((row) => row.anchor)).toEqual([null, undefined]);
    expect(setProjectViewAnchor(list, "nope", "abc")?.map((row) => row.anchor)).toEqual([undefined, undefined]);
  });

  it("passes a missing list through every patch", () => {
    expect(flipViewFavorite(undefined, VIEW_ID)).toBeUndefined();
    expect(replaceProjectViewRow(undefined, makeView())).toBeUndefined();
    expect(removeProjectViewRow(undefined, VIEW_ID)).toBeUndefined();
    expect(setProjectViewAnchor(undefined, VIEW_ID, "abc")).toBeUndefined();
  });

  it("adopts an anchor into a detail record and passes a missing one through", () => {
    expect(withViewAnchor(makeView(), "abc")?.anchor).toBe("abc");
    expect(withViewAnchor(makeView({ anchor: "abc" }), null)?.anchor).toBeNull();
    expect(withViewAnchor(undefined, "abc")).toBeUndefined();
  });
});

function favorite(overrides: Partial<Favorite> = {}): Favorite {
  return {
    id: "723e4567-e89b-12d3-a456-426614174006",
    entity_type: "view",
    entity_identifier: VIEW_ID,
    ...overrides,
  } as Favorite;
}

describe("findViewFavoriteId", () => {
  it("returns the view favorite row's own id", () => {
    expect(findViewFavoriteId([favorite()], VIEW_ID)).toBe("723e4567-e89b-12d3-a456-426614174006");
  });

  it("ignores other entities and views, and misses read null", () => {
    const rows = [
      favorite({ id: "other", entity_type: "project" }),
      favorite({ id: "else", entity_identifier: OTHER_ID }),
      favorite({ id: "null-id", entity_identifier: null }),
    ];
    expect(findViewFavoriteId(rows, VIEW_ID)).toBeNull();
    expect(findViewFavoriteId([], VIEW_ID)).toBeNull();
  });
});
