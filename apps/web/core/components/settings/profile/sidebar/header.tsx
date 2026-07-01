/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { ArrowLeft } from "lucide-react";
import { observer } from "mobx-react";
import { useParams } from "react-router";
// pi dash imports
import { useTranslation } from "@pi-dash/i18n";
import { IconButton } from "@pi-dash/propel/icon-button";
import { Avatar } from "@pi-dash/ui";
// hooks
import { useUser } from "@/hooks/store/user";
import { useWorkspace } from "@/hooks/store/use-workspace";
import { useAppRouter } from "@/hooks/use-app-router";
import { getFileURL } from "@pi-dash/utils";

export const ProfileSettingsSidebarHeader = observer(function ProfileSettingsSidebarHeader() {
  // router
  const router = useAppRouter();
  // params — `profileTabId` is only present on the standalone /settings page, not the modal
  const { profileTabId } = useParams();
  // store hooks
  const { data: currentUser } = useUser();
  const { getWorkspaceRedirectionUrl } = useWorkspace();
  // translation
  const { t } = useTranslation();

  return (
    <div className="flex shrink-0 flex-col gap-2">
      {/* Back to home — only on the standalone settings page; the modal has its own close button */}
      {profileTabId && (
        <div className="flex items-center gap-1 text-body-md-medium">
          <IconButton
            variant="ghost"
            size="base"
            icon={ArrowLeft}
            aria-label={t("Back to home")}
            onClick={() => router.push(getWorkspaceRedirectionUrl())}
          />
          <p>{t("Profile settings")}</p>
        </div>
      )}
      <div className="flex items-center gap-2">
        <div className="shrink-0">
          <Avatar
            src={getFileURL(currentUser?.avatar_url ?? "")}
            name={currentUser?.display_name}
            size={32}
            shape="circle"
            className="text-16"
          />
        </div>
        <div className="truncate">
          <p className="truncate text-body-sm-medium">
            {currentUser?.first_name} {currentUser?.last_name}
          </p>
          <p className="truncate text-caption-md-regular">{currentUser?.email}</p>
        </div>
      </div>
    </div>
  );
});
