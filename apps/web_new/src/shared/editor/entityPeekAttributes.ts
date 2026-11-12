// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Stored entity-reference attributes to a shell peek request (PAGE-043,
// §13.2). Both the editor node view and the read surface use this one
// conversion, so a preview card's click can never disagree with the
// identifiers the document saved.

import type { EntityPeekRequest } from "../shell/entityPeek.js";

/** Build a peek request from a reference's stored attributes. */
export function parseEntityPeekAttributes(
  attrs: Record<string, unknown>,
  fallbackLabel = ""
): EntityPeekRequest | null {
  const entityName = stringOf(attrs["entity_name"]);
  const entityIdentifier = stringOf(attrs["entity_identifier"]);
  if (entityName === "" || entityIdentifier === "") return null;
  const projectId = stringOf(attrs["project_id"]);
  return {
    entityName,
    entityIdentifier,
    projectIdentifier: stringOf(attrs["project_identifier"]),
    workspaceIdentifier: stringOf(attrs["workspace_identifier"]),
    projectId: projectId === "" ? null : projectId,
    label: stringOf(attrs["data-label"]) || fallbackLabel || entityIdentifier,
  };
}

function stringOf(value: unknown): string {
  return typeof value === "string" ? value : "";
}
