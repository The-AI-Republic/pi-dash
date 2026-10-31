// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Stored entity reference to shell peek request (PAGE-043, §13.2).

import { describe, expect, it } from "vitest";

import { parseEntityPeekAttributes } from "./entityPeekAttributes.js";

describe("parseEntityPeekAttributes", () => {
  it("maps every stored identifier onto the peek request", () => {
    expect(
      parseEntityPeekAttributes({
        entity_name: "issue",
        entity_identifier: "iss-1",
        project_identifier: "WEB",
        workspace_identifier: "acme",
        project_id: "project-1",
        "data-label": "Fix login",
      })
    ).toEqual({
      entityName: "issue",
      entityIdentifier: "iss-1",
      projectIdentifier: "WEB",
      workspaceIdentifier: "acme",
      projectId: "project-1",
      label: "Fix login",
    });
  });

  it("uses the fallback label and a null project when they are absent", () => {
    expect(parseEntityPeekAttributes({ entity_name: "issue", entity_identifier: "iss-1" }, "Fallback")).toMatchObject({
      projectId: null,
      label: "Fallback",
    });
  });

  it("falls back to the identifier and refuses a reference without identity", () => {
    expect(parseEntityPeekAttributes({ entity_name: "issue", entity_identifier: "iss-1" })?.label).toBe("iss-1");
    expect(parseEntityPeekAttributes({ entity_name: "issue" })).toBeNull();
    expect(parseEntityPeekAttributes({ entity_identifier: "iss-1" })).toBeNull();
    expect(parseEntityPeekAttributes({ entity_name: 7, entity_identifier: 8 })).toBeNull();
  });
});
