/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { IEditorPropsExtended } from "@pi-dash/editor";
import type { TSearchEntityRequestPayload, TSearchResponse } from "@pi-dash/types";
import type { TPageInstance } from "@/store/pages/base-page";
import type { EPageStoreType } from "../store";

export type TExtendedEditorExtensionsHookParams = {
  workspaceSlug: string;
  page: TPageInstance;
  storeType: EPageStoreType;
  fetchEntity: (payload: TSearchEntityRequestPayload) => Promise<TSearchResponse>;
  getRedirectionLink: (pageId?: string) => string;
  extensionHandlers?: Map<string, unknown>;
  projectId?: string;
};

export type TExtendedEditorExtensionsConfig = IEditorPropsExtended;

export const useExtendedEditorProps = (
  _params: TExtendedEditorExtensionsHookParams
): TExtendedEditorExtensionsConfig => ({});
