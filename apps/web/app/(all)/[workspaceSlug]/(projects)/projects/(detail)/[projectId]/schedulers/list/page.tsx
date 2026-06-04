/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { useTranslation } from "@pi-dash/i18n";
import { PageHead } from "@/components/core/page-title";
import { SchedulerBindingsPanel } from "@/components/project/scheduler-bindings/bindings-panel";
import { useProject } from "@/hooks/store/use-project";
import type { Route } from "./+types/page";

function ProjectSchedulersListPage({ params }: Route.ComponentProps) {
  const { workspaceSlug, projectId } = params;
  const { t } = useTranslation();
  const { getProjectById } = useProject();
  const project = getProjectById(projectId);
  const pageTitle = project?.name
    ? `${project.name} - ${t("Schedulers")}`
    : t("Schedulers");

  return (
    <>
      <PageHead title={pageTitle} />
      <SchedulerBindingsPanel workspaceSlug={workspaceSlug} projectId={projectId} />
    </>
  );
}

export default observer(ProjectSchedulersListPage);
