// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Read-only stored-HTML render (UI kit and editor page). No Tiptap:
// lists, peek, comments and activity show descriptions through this
// and load the editor chunk only when the user starts editing.

import * as React from "react";

import { renderableHtml } from "./sanitize.js";
import { mentionDirectoryVersion, subscribeMentionDirectory } from "./mentionDirectory.js";
import { parseEntityPeekAttributes } from "./entityPeekAttributes.js";
import { requestEntityPeek } from "../shell/entityPeek.js";

export interface RichTextViewProps {
  /** Stored description or comment HTML. */
  html: string;
  /** Accessible label for the rendered region. */
  label: string;
  className?: string | undefined;
  /** Resolve a stored image asset id to its served source (ISS-127). */
  resolveAsset?: ((src: string) => string) | undefined;
  /** Workspace slug: person mentions link to the member profile (CMT-020). */
  workspaceSlug?: string | undefined;
}

export function RichTextView({
  html,
  label,
  className,
  resolveAsset,
  workspaceSlug,
}: RichTextViewProps): React.ReactElement {
  // A mention chip resolves the person's current name from the directory
  // (PAGE-041), so the read surface repaints when that directory arrives.
  const directoryVersion = React.useSyncExternalStore(subscribeMentionDirectory, mentionDirectoryVersion);
  // Stored bodies carry editor nodes (mentions, image components) that
  // have no paintable markup of their own; the renderable form turns
  // each into a chip or an image before it reaches the frame (ISS-127).
  const clean = React.useMemo(
    () => renderableHtml(html, resolveAsset, workspaceSlug),
    [html, resolveAsset, workspaceSlug, directoryVersion]
  );
  const classes = className === undefined ? "rich-text-content" : `rich-text-content ${className}`;
  // A reference preview's stored identifiers are copied onto its rendered
  // span; activation forwards them to the shell peek (PAGE-043).
  const activatePreview = (target: EventTarget | null): void => {
    const ElementConstructor = globalThis.Element;
    if (ElementConstructor === undefined || !(target instanceof ElementConstructor)) return;
    const preview = target.closest("[data-entity-preview]");
    if (preview === null) return;
    const request = parseEntityPeekAttributes(
      {
        entity_name: preview.getAttribute("data-entity-name"),
        entity_identifier: preview.getAttribute("data-entity-identifier"),
        project_identifier: preview.getAttribute("data-project-identifier"),
        workspace_identifier: preview.getAttribute("data-workspace-identifier"),
        project_id: preview.getAttribute("data-project-id"),
        "data-label": preview.textContent,
      },
      preview.textContent ?? ""
    );
    if (request !== null) requestEntityPeek(request);
  };
  return (
    <div
      aria-label={label}
      role="region"
      className={classes}
      onClick={(event) => activatePreview(event.target)}
      onKeyDown={(event) => {
        if (event.key !== "Enter" && event.key !== " ") return;
        activatePreview(event.target);
      }}
      dangerouslySetInnerHTML={{ __html: clean }}
    />
  );
}
