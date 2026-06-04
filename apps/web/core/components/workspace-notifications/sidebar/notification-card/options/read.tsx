/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { MessageSquare } from "lucide-react";
// pi dash imports
import { useTranslation } from "@pi-dash/i18n";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
// store
import type { INotification } from "@/store/notifications/notification";
// local imports
import { NotificationItemOptionButton } from "./button";

type TNotificationItemReadOption = {
  workspaceSlug: string;
  notification: INotification;
};

export const NotificationItemReadOption = observer(function NotificationItemReadOption(
  props: TNotificationItemReadOption
) {
  const { workspaceSlug, notification } = props;
  // hooks
  const { asJson: data, markNotificationAsRead, markNotificationAsUnRead } = notification;
  const { t } = useTranslation();

  const handleNotificationUpdate = async () => {
    try {
      const request = data.read_at ? markNotificationAsUnRead : markNotificationAsRead;
      await request(workspaceSlug);
      setToast({
        title: data.read_at ? t("Notification marked as unread") : t("Notification marked as read"),
        type: TOAST_TYPE.SUCCESS,
      });
    } catch (e) {
      console.error(e);
    }
  };

  return (
    <NotificationItemOptionButton
      tooltipContent={data.read_at ? t("Mark as unread") : t("Mark as read")}
      callBack={handleNotificationUpdate}
    >
      <MessageSquare className="h-3 w-3 text-tertiary" />
    </NotificationItemOptionButton>
  );
});
