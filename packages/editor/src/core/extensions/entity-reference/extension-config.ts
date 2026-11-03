/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { mergeAttributes, Node } from "@tiptap/core";
// local types
import { EEntityReferenceAttributeNames } from "./types";

/**
 * Server-side schema for the stored `entity-reference-component` node
 * (PAGE-043): an inline atom carrying the identifiers a peek overview
 * resolves, with the display text in `data-label` and no text content.
 * Used only by the document conversion extensions so saved pages keep
 * the node; it has no client node view.
 */
export const EntityReferenceExtensionConfig = Node.create({
  name: "entity-reference-component",
  group: "inline",
  inline: true,
  atom: true,
  selectable: true,

  addAttributes() {
    return {
      [EEntityReferenceAttributeNames.ENTITY_NAME]: {
        default: undefined,
      },
      [EEntityReferenceAttributeNames.ENTITY_IDENTIFIER]: {
        default: undefined,
      },
      [EEntityReferenceAttributeNames.PROJECT_IDENTIFIER]: {
        default: undefined,
      },
      [EEntityReferenceAttributeNames.WORKSPACE_IDENTIFIER]: {
        default: undefined,
      },
      [EEntityReferenceAttributeNames.PROJECT_ID]: {
        default: undefined,
      },
      [EEntityReferenceAttributeNames.LABEL]: {
        default: undefined,
      },
    };
  },

  parseHTML() {
    return [
      {
        tag: "entity-reference-component",
      },
    ];
  },

  renderHTML({ HTMLAttributes }) {
    return ["entity-reference-component", mergeAttributes(HTMLAttributes)];
  },
});
