/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
// pi dash imports
import { useTranslation } from "@pi-dash/i18n";
// components
import { PageHead } from "@/components/core/page-title";
import { NotificationsRoot } from "@/components/workspace-notifications";
// hooks
import { useWorkspace } from "@/hooks/store/use-workspace";
import type { Route } from "./+types/page";

function WorkspaceDashboardPage({ params }: Route.ComponentProps) {
  const { workspaceSlug } = params;
  // pi dash hooks
  const { t } = useTranslation();
  // hooks
  const { currentWorkspace } = useWorkspace();
  // derived values
  const pageTitle = currentWorkspace?.name
    ? t("notification.page_label", { workspace: currentWorkspace?.name })
    : undefined;

  return (
    <>
      <PageHead title={pageTitle} />
      <NotificationsRoot workspaceSlug={workspaceSlug} />
    </>
  );
}

export default observer(WorkspaceDashboardPage);
