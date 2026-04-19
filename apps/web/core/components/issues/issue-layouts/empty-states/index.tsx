/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

// pi dash web components
import { EIssuesStoreType } from "@pi-dash/types";
import { TeamEmptyState } from "@/pi-dash-web/components/issues/issue-layouts/empty-states/team-issues";
import { TeamProjectWorkItemEmptyState } from "@/pi-dash-web/components/issues/issue-layouts/empty-states/team-project";
import { TeamViewEmptyState } from "@/pi-dash-web/components/issues/issue-layouts/empty-states/team-view-issues";
// components
import { ProjectArchivedEmptyState } from "./archived-issues";
import { CycleEmptyState } from "./cycle";
import { GlobalViewEmptyState } from "./global-view";
import { ModuleEmptyState } from "./module";
import { ProfileViewEmptyState } from "./profile-view";
import { ProjectEpicsEmptyState } from "./project-epic";
import { ProjectEmptyState } from "./project-issues";
import { ProjectViewEmptyState } from "./project-view";

interface Props {
  storeType: EIssuesStoreType;
}

export function IssueLayoutEmptyState(props: Props) {
  switch (props.storeType) {
    case EIssuesStoreType.PROJECT:
      return <ProjectEmptyState />;
    case EIssuesStoreType.PROJECT_VIEW:
      return <ProjectViewEmptyState />;
    case EIssuesStoreType.ARCHIVED:
      return <ProjectArchivedEmptyState />;
    case EIssuesStoreType.CYCLE:
      return <CycleEmptyState />;
    case EIssuesStoreType.MODULE:
      return <ModuleEmptyState />;
    case EIssuesStoreType.GLOBAL:
      return <GlobalViewEmptyState />;
    case EIssuesStoreType.PROFILE:
      return <ProfileViewEmptyState />;
    case EIssuesStoreType.EPIC:
      return <ProjectEpicsEmptyState />;
    case EIssuesStoreType.TEAM:
      return <TeamEmptyState />;
    case EIssuesStoreType.TEAM_VIEW:
      return <TeamViewEmptyState />;
    case EIssuesStoreType.TEAM_PROJECT_WORK_ITEMS:
      return <TeamProjectWorkItemEmptyState />;
    default:
      return null;
  }
}
