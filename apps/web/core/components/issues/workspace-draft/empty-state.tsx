/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { Fragment, useState } from "react";
// components
import { observer } from "mobx-react";
import { EUserPermissionsLevel } from "@apple-pi-dash/constants";
import { useTranslation } from "@apple-pi-dash/i18n";
import { EmptyStateDetailed } from "@apple-pi-dash/propel/empty-state";
import { EIssuesStoreType, EUserWorkspaceRoles } from "@apple-pi-dash/types";
import { CreateUpdateIssueModal } from "@/components/issues/issue-modal/modal";
// constants
import { useUserPermissions } from "@/hooks/store/user";

export const WorkspaceDraftEmptyState = observer(function WorkspaceDraftEmptyState() {
  // state
  const [isDraftIssueModalOpen, setIsDraftIssueModalOpen] = useState(false);
  // store hooks
  const { t } = useTranslation();
  const { allowPermissions } = useUserPermissions();
  // derived values
  const canPerformEmptyStateActions = allowPermissions(
    [EUserWorkspaceRoles.ADMIN, EUserWorkspaceRoles.MEMBER],
    EUserPermissionsLevel.WORKSPACE
  );

  return (
    <Fragment>
      <CreateUpdateIssueModal
        isOpen={isDraftIssueModalOpen}
        storeType={EIssuesStoreType.WORKSPACE_DRAFT}
        onClose={() => setIsDraftIssueModalOpen(false)}
        isDraft
      />
      <div className="relative h-full w-full overflow-y-auto">
        <EmptyStateDetailed
          title={t("workspace_empty_state.drafts.title")}
          description={t("workspace_empty_state.drafts.description")}
          assetKey="draft"
          actions={[
            {
              label: t("workspace_empty_state.drafts.cta_primary"),
              onClick: () => {
                setIsDraftIssueModalOpen(true);
              },
              disabled: !canPerformEmptyStateActions,
              variant: "primary",
            },
          ]}
        />
      </div>
    </Fragment>
  );
});
