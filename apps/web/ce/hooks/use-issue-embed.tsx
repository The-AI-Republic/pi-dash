/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

// editor
import type { TEmbedConfig } from "@apple-pi-dash/editor";
// apple pi dash types
import type { TSearchEntityRequestPayload, TSearchResponse } from "@apple-pi-dash/types";
// apple pi dash web components
import { IssueEmbedUpgradeCard } from "@/apple-pi-dash-web/components/pages";

export type TIssueEmbedHookProps = {
  fetchEmbedSuggestions?: (payload: TSearchEntityRequestPayload) => Promise<TSearchResponse>;
  projectId?: string;
  workspaceSlug?: string;
};

// eslint-disable-next-line @typescript-eslint/no-unused-vars
export const useIssueEmbed = (props: TIssueEmbedHookProps) => {
  const widgetCallback = () => <IssueEmbedUpgradeCard />;

  const issueEmbedProps: TEmbedConfig["issue"] = {
    widgetCallback,
  };

  return {
    issueEmbedProps,
  };
};
