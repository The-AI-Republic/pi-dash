/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useCallback } from "react";
// apple pi dash imports
import type { TExtendedFileHandler } from "@apple-pi-dash/editor";

export type TExtendedEditorFileHandlersArgs = {
  projectId?: string;
  workspaceSlug: string;
};

export type TExtendedEditorConfig = {
  getExtendedEditorFileHandlers: (args: TExtendedEditorFileHandlersArgs) => TExtendedFileHandler;
};

export const useExtendedEditorConfig = (): TExtendedEditorConfig => {
  const getExtendedEditorFileHandlers: TExtendedEditorConfig["getExtendedEditorFileHandlers"] = useCallback(
    () => ({}),
    []
  );

  return {
    getExtendedEditorFileHandlers,
  };
};
