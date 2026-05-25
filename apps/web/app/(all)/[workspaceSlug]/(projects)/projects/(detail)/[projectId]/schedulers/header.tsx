/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { useParams } from "next/navigation";
import { CalendarClock } from "lucide-react";
import { useTranslation } from "@pi-dash/i18n";
import { Breadcrumbs, Header } from "@pi-dash/ui";
import { BreadcrumbLink } from "@/components/common/breadcrumb-link";
import { useProject } from "@/hooks/store/use-project";
import { CommonProjectBreadcrumbs } from "@/pi-dash-web/components/breadcrumbs/common";

export const ProjectSchedulersHeader = observer(function ProjectSchedulersHeader() {
  const { workspaceSlug, projectId } = useParams();
  const { currentProjectDetails, loader } = useProject();
  const { t } = useTranslation();

  return (
    <Header>
      <Header.LeftItem>
        <Breadcrumbs isLoading={loader === "init-loader"}>
          <CommonProjectBreadcrumbs workspaceSlug={workspaceSlug?.toString()} projectId={projectId?.toString()} />
          <Breadcrumbs.Item
            component={
              <BreadcrumbLink
                label={t("sidebar.schedulers")}
                href={`/${workspaceSlug}/projects/${currentProjectDetails?.id}/schedulers`}
                icon={<CalendarClock className="size-4 text-tertiary" />}
                isLast
              />
            }
            isLast
          />
        </Breadcrumbs>
      </Header.LeftItem>
    </Header>
  );
});
