// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { resolveFavoriteHref, resolveFavoriteIconKind } from "./favoriteLinks.js";

describe("resolveFavoriteIconKind", () => {
  it("renders folders as folders however they are typed", () => {
    expect(resolveFavoriteIconKind({ entity_type: "folder", is_folder: true })).toBe("folder");
    expect(resolveFavoriteIconKind({ entity_type: "project", is_folder: true })).toBe("folder");
  });

  it("renders known entity types with their own glyph", () => {
    expect(resolveFavoriteIconKind({ entity_type: "project", is_folder: false })).toBe("project");
    expect(resolveFavoriteIconKind({ entity_type: "page", is_folder: false })).toBe("page");
    expect(resolveFavoriteIconKind({ entity_type: "view", is_folder: false })).toBe("view");
    expect(resolveFavoriteIconKind({ entity_type: "module", is_folder: false })).toBe("module");
    expect(resolveFavoriteIconKind({ entity_type: "cycle", is_folder: false })).toBe("cycle");
  });

  it("falls back for unknown types instead of crashing", () => {
    expect(resolveFavoriteIconKind({ entity_type: "teleporter", is_folder: false })).toBe("fallback");
  });
});

describe("resolveFavoriteHref", () => {
  it("lands projects on tracking", () => {
    expect(resolveFavoriteHref("acme", { entity_type: "project", entity_identifier: "p1", project_id: "p1" })).toBe(
      "/acme/projects/p1/issues"
    );
  });

  it("lands cycles, modules, views and pages on their detail page", () => {
    expect(resolveFavoriteHref("acme", { entity_type: "cycle", entity_identifier: "c1", project_id: "p1" })).toBe(
      "/acme/projects/p1/cycles/c1"
    );
    expect(resolveFavoriteHref("acme", { entity_type: "module", entity_identifier: "m1", project_id: "p1" })).toBe(
      "/acme/projects/p1/modules/m1"
    );
    expect(resolveFavoriteHref("acme", { entity_type: "view", entity_identifier: "v1", project_id: "p1" })).toBe(
      "/acme/projects/p1/views/v1"
    );
    expect(resolveFavoriteHref("acme", { entity_type: "page", entity_identifier: "g1", project_id: "p1" })).toBe(
      "/acme/projects/p1/pages/g1"
    );
  });

  it("falls back to the workspace root for unknown types", () => {
    expect(resolveFavoriteHref("acme", { entity_type: "teleporter", entity_identifier: "t1", project_id: "p1" })).toBe(
      "/acme"
    );
  });

  it("falls back when identifiers are missing", () => {
    expect(resolveFavoriteHref("acme", { entity_type: "project", entity_identifier: "p1", project_id: null })).toBe(
      "/acme"
    );
    expect(resolveFavoriteHref("acme", { entity_type: "cycle", entity_identifier: null, project_id: "p1" })).toBe(
      "/acme"
    );
  });
});
