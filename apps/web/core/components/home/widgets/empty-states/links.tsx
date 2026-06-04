/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useTranslation } from "@pi-dash/i18n";
import { EmptyStateCompact } from "@pi-dash/propel/empty-state";

export function LinksEmptyState() {
  const { t } = useTranslation();
  return (
    <div className="flex w-full items-center justify-center rounded-lg bg-layer-1 py-10">
      <EmptyStateCompact
        assetKey="link"
        assetClassName="w-20 h-20"
        title={t("Keep important references, resources, or docs handy for your work")}
      />
    </div>
  );
}
