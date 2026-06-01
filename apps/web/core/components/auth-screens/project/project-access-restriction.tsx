/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
// pi dash imports
import { useTranslation } from "@pi-dash/i18n";
import { EmptyStateDetailed } from "@pi-dash/propel/empty-state";

type TProps = {
  isWorkspaceAdmin: boolean;
  handleJoinProject: () => void;
  isJoinButtonDisabled: boolean;
  errorStatusCode: number | undefined;
};

export const ProjectAccessRestriction = observer(function ProjectAccessRestriction(props: TProps) {
  const { isWorkspaceAdmin, handleJoinProject, isJoinButtonDisabled, errorStatusCode } = props;
  // pi dash hooks
  const { t } = useTranslation();

  // Show join project screen if:
  // - User lacks project membership (409 Conflict)
  // - User lacks permission to access the private project (403 Forbidden) but is a workspace admin (can join any project)
  if (errorStatusCode === 409 || (errorStatusCode === 403 && isWorkspaceAdmin))
    return (
      <div className="grid h-full w-full place-items-center bg-surface-1">
        <EmptyStateDetailed
          title={t("Seems like you don’t have access to this Project")}
          description={t("Click the button below to join it.")}
          assetKey="no-access"
          assetClassName="size-40"
          actions={[
            {
              label: isJoinButtonDisabled
                ? t("Joining project")
                : t("Join project"),
              onClick: handleJoinProject,
              disabled: isJoinButtonDisabled,
            },
          ]}
        />
      </div>
    );

  // Show no access screen if:
  // - User lacks permission to access the private project (403 Forbidden)
  if (errorStatusCode === 403) {
    return (
      <div className="grid h-full w-full place-items-center bg-surface-1">
        <EmptyStateDetailed
          title={t("Seems like you don’t have access to this Project")}
          description={t("Contact admin to request for access and you can continue here.")}
          assetKey="no-access"
          assetClassName="size-40"
        />
      </div>
    );
  }

  // Show empty state screen if:
  // - Project not found (404 Not Found)
  // - Any other error status code
  return (
    <div className="grid h-full w-full place-items-center bg-surface-1">
      <EmptyStateDetailed
        title={t("Project not found")}
        description={t("The project you are looking for does not exist.")}
        assetKey="project"
        assetClassName="size-40"
      />
    </div>
  );
});
