/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useTranslation } from "@pi-dash/i18n";
import { EmptyStateCompact } from "@pi-dash/propel/empty-state";
import type { CompactAssetType } from "@pi-dash/propel/empty-state";

const getDisplayContent = (type: string): { assetKey: CompactAssetType; text: string } => {
  switch (type) {
    case "project":
      return {
        assetKey: "project",
        text: "Your recent projects will appear here once you visit one.",
      };
    case "page":
      return {
        assetKey: "note",
        text: "Your recent pages will appear here once you visit one.",
      };
    case "issue":
      return {
        assetKey: "work-item",
        text: "Your recent work items will appear here once you visit one.",
      };
    default:
      return {
        assetKey: "work-item",
        text: "You don't have any recents yet.",
      };
  }
};

export function RecentsEmptyState({ type }: { type: string }) {
  const { t } = useTranslation();

  const { assetKey, text } = getDisplayContent(type);

  return (
    <div className="flex w-full items-center justify-center rounded-lg bg-layer-1 py-10">
      <EmptyStateCompact assetKey={assetKey} assetClassName="size-20" title={t(text)} />
    </div>
  );
}
