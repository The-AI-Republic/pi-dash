/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect } from "react";
import { observer } from "mobx-react";
import { useTheme } from "next-themes";
import useSWR from "swr";
// pi dash imports
import { useTranslation } from "@pi-dash/i18n";
import type { TIssue } from "@pi-dash/types";
import { EIssueServiceType } from "@pi-dash/types";
import { Loader } from "@pi-dash/ui";
// assets
import emptyIssueDark from "@/app/assets/empty-state/search/issues-dark.webp?url";
import emptyIssueLight from "@/app/assets/empty-state/search/issues-light.webp?url";
// components
import { EmptyState } from "@/components/common/empty-state";
import { PageHead } from "@/components/core/page-title";
// hooks
import { useAppTheme } from "@/hooks/store/use-app-theme";
import { useIssueDetail } from "@/hooks/store/use-issue-detail";
import { useProject } from "@/hooks/store/use-project";
import { useAppRouter } from "@/hooks/use-app-router";
import type { TIssueLite } from "@/store/issue/issue-details/issue.store";
// layouts
import { ProjectAuthWrapper } from "@/layouts/auth-layout/project-wrapper";
// pi dash web imports
import { useWorkItemProperties } from "@/pi-dash-web/hooks/use-issue-properties";
import { WorkItemDetailRoot } from "@/pi-dash-web/components/browse/workItem-detail";

import type { Route } from "./+types/page";

export const IssueDetailsPage = observer(function IssueDetailsPage({ params }: Route.ComponentProps) {
  // router
  const router = useAppRouter();
  const { workspaceSlug, workItem } = params;
  // hooks
  const { resolvedTheme } = useTheme();
  // store hooks
  const { t } = useTranslation();
  const {
    fetchIssue,
    fetchIssueWithIdentifier,
    issue: { getIssueById },
  } = useIssueDetail();
  const { fetchIssue: fetchEpic } = useIssueDetail(EIssueServiceType.EPICS);
  const { getProjectById, getProjectByIdentifier } = useProject();
  const { toggleIssueDetailSidebar, issueDetailSidebarCollapsed } = useAppTheme();

  const [projectIdentifier, sequence_id] = workItem.split("-");

  // fetching issue details
  const {
    data: liteIssue,
    isLoading,
    error,
  } = useSWR<TIssueLite, Error>(`ISSUE_DETAIL_${workspaceSlug}_${projectIdentifier}_${sequence_id}`, () =>
    fetchIssueWithIdentifier(workspaceSlug.toString(), projectIdentifier, sequence_id, { lite: true })
  );

  // derived values
  const projectDetails = getProjectByIdentifier(projectIdentifier);
  const issueId = liteIssue?.id;
  const projectId = liteIssue?.project_id ?? projectDetails?.id ?? "";
  const issue = getIssueById(issueId?.toString() || "") || undefined;
  const project = (issue?.project_id && getProjectById(issue?.project_id)) || undefined;
  const issueLoader = !issue || isLoading;
  const pageTitle = project && issue ? `${project?.identifier}-${issue?.sequence_id} ${issue?.name}` : undefined;
  const shouldHydrateIssue = !!workspaceSlug && !!projectId && !!issueId && !liteIssue?.is_intake;

  const {
    data: hydratedIssue,
    isLoading: isHydratingIssue,
    error: hydrationError,
    mutate: retryIssueHydration,
  } = useSWR<TIssue, Error>(
    shouldHydrateIssue ? `ISSUE_DETAIL_HYDRATE_${workspaceSlug}_${projectId}_${issueId}` : null,
    () =>
      (liteIssue?.is_epic ? fetchEpic : fetchIssue)(
        workspaceSlug.toString(),
        projectId.toString(),
        issueId?.toString() ?? "",
        {
          preserveSubscription: true,
          skipActivityAndComments: true,
        }
      ),
    { revalidateOnFocus: false }
  );
  const isMetadataHydrating = shouldHydrateIssue && isHydratingIssue && !hydratedIssue;

  useWorkItemProperties(
    projectId,
    workspaceSlug.toString(),
    issueId,
    issue?.is_epic ? EIssueServiceType.EPICS : EIssueServiceType.ISSUES
  );

  useEffect(() => {
    const handleToggleIssueDetailSidebar = () => {
      if (window && window.innerWidth < 768) {
        toggleIssueDetailSidebar(true);
      }
      if (window && issueDetailSidebarCollapsed && window.innerWidth >= 768) {
        toggleIssueDetailSidebar(false);
      }
    };
    window.addEventListener("resize", handleToggleIssueDetailSidebar);
    handleToggleIssueDetailSidebar();
    return () => window.removeEventListener("resize", handleToggleIssueDetailSidebar);
  }, [issueDetailSidebarCollapsed, toggleIssueDetailSidebar]);

  useEffect(() => {
    if (liteIssue?.is_intake) {
      router.push(
        `/${workspaceSlug}/projects/${liteIssue.project_id}/intake/?currentTab=open&inboxIssueId=${liteIssue?.id}`
      );
    }
  }, [workspaceSlug, liteIssue?.id, liteIssue?.is_intake, liteIssue?.project_id, router]);

  if ((error && !isLoading) || hydrationError) {
    return (
      <EmptyState
        image={resolvedTheme === "dark" ? emptyIssueDark : emptyIssueLight}
        title={t("issue.empty_state.issue_detail.title")}
        description={t("issue.empty_state.issue_detail.description")}
        primaryButton={{
          text: hydrationError ? t("common.retry") : t("issue.empty_state.issue_detail.primary_button.text"),
          onClick: () => {
            if (hydrationError) void retryIssueHydration();
            else router.push(`/${workspaceSlug}/workspace-views/all-issues/`);
          },
        }}
      />
    );
  }

  if (issueLoader) {
    return (
      <Loader className="flex h-full gap-5 p-5">
        <div className="basis-2/3 space-y-2">
          <Loader.Item height="30px" width="40%" />
          <Loader.Item height="15px" width="60%" />
          <Loader.Item height="15px" width="60%" />
          <Loader.Item height="15px" width="40%" />
        </div>
        <div className="basis-1/3 space-y-3">
          <Loader.Item height="30px" />
          <Loader.Item height="30px" />
          <Loader.Item height="30px" />
          <Loader.Item height="30px" />
        </div>
      </Loader>
    );
  }

  return (
    <>
      <PageHead title={pageTitle} />
      {workspaceSlug && projectId && issueId && (
        <ProjectAuthWrapper workspaceSlug={workspaceSlug} projectId={projectId}>
          <WorkItemDetailRoot
            workspaceSlug={workspaceSlug.toString()}
            projectId={projectId.toString()}
            issueId={issueId.toString()}
            issue={issue}
            isMetadataHydrating={isMetadataHydrating}
          />
        </ProjectAuthWrapper>
      )}
    </>
  );
});

export default IssueDetailsPage;
