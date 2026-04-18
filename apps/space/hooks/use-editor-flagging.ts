/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

// editor
import type { TExtensions } from "@apple-pi-dash/editor";

export type TEditorFlaggingHookReturnType = {
  document: {
    disabled: TExtensions[];
    flagged: TExtensions[];
  };
  liteText: {
    disabled: TExtensions[];
    flagged: TExtensions[];
  };
  richText: {
    disabled: TExtensions[];
    flagged: TExtensions[];
  };
};

/**
 * @description extensions disabled in various editors
 */
export const useEditorFlagging = (_anchor: string): TEditorFlaggingHookReturnType => ({
  document: {
    disabled: [],
    flagged: [],
  },
  liteText: {
    disabled: [],
    flagged: [],
  },
  richText: {
    disabled: [],
    flagged: [],
  },
});
