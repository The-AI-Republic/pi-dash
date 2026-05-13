/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { useParams } from "next/navigation";
// pi dash imports
import { useTranslation } from "@pi-dash/i18n";
import { CalendarAfterIcon } from "@pi-dash/propel/icons";
import { Breadcrumbs, Header } from "@pi-dash/ui";
// components
import { BreadcrumbLink } from "@/components/common/breadcrumb-link";
// hooks
import { useProject } from "@/hooks/store/use-project";
// pi dash web imports
import { CommonProjectBreadcrumbs } from "@/pi-dash-web/components/breadcrumbs/common";

export const ProjectSchedulersHeader = observer(function ProjectSchedulersHeader() {
  const { workspaceSlug, projectId } = useParams();
  const { t } = useTranslation();
  const { loader: currentProjectDetailsLoader } = useProject();

  return (
    <Header>
      <Header.LeftItem>
        <div className="flex flex-grow items-center gap-4">
          <Breadcrumbs isLoading={currentProjectDetailsLoader === "init-loader"}>
            <CommonProjectBreadcrumbs workspaceSlug={workspaceSlug?.toString()} projectId={projectId?.toString()} />
            <Breadcrumbs.Item
              component={
                <BreadcrumbLink
                  label={t("scheduler_bindings.tab_label")}
                  href={`/${workspaceSlug}/projects/${projectId}/schedulers/`}
                  icon={<CalendarAfterIcon className="h-4 w-4 text-tertiary" />}
                  isLast
                />
              }
              isLast
            />
          </Breadcrumbs>
        </div>
      </Header.LeftItem>
    </Header>
  );
});
