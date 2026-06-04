/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { useTranslation } from "@pi-dash/i18n";
import { PageHead } from "@/components/core/page-title";
import { SchedulerCalendar } from "@/components/project/scheduler-bindings/calendar/scheduler-calendar";
import { useProject } from "@/hooks/store/use-project";
import type { Route } from "./+types/page";

function ProjectSchedulersCalendarPage({ params }: Route.ComponentProps) {
  const { workspaceSlug, projectId } = params;
  const { t } = useTranslation();
  const { getProjectById } = useProject();
  const project = getProjectById(projectId);
  const pageTitle = project?.name
    ? `${project.name} - ${t("Schedulers")} (${t("Calendar")})`
    : t("Calendar");

  return (
    <>
      <PageHead title={pageTitle} />
      <div className="h-[calc(100vh-8rem)]">
        <SchedulerCalendar workspaceSlug={workspaceSlug} projectId={projectId} />
      </div>
    </>
  );
}

export default observer(ProjectSchedulersCalendarPage);
