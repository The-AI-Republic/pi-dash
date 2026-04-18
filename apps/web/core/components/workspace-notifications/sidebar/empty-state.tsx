/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
// apple pi dash imports
import { ENotificationTab } from "@apple-pi-dash/constants";
import { useTranslation } from "@apple-pi-dash/i18n";
import { EmptyStateCompact } from "@apple-pi-dash/propel/empty-state";

type TNotificationEmptyStateProps = {
  currentNotificationTab: ENotificationTab;
};

export const NotificationEmptyState = observer(function NotificationEmptyState({
  currentNotificationTab,
}: TNotificationEmptyStateProps) {
  // apple pi dash imports
  const { t } = useTranslation();

  return (
    <>
      <EmptyStateCompact
        assetKey="inbox"
        assetClassName="size-24"
        title={
          currentNotificationTab === ENotificationTab.ALL
            ? t("workspace_empty_state.inbox_sidebar_all.title")
            : t("workspace_empty_state.inbox_sidebar_mentions.title")
        }
        className="max-w-56"
      />
    </>
  );
});
