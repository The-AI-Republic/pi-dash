/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { pull, concat, uniq, set, update } from "lodash-es";
import { action, makeObservable, observable, runInAction } from "mobx";
import { computedFn } from "mobx-utils";
// Pi Dash Imports
import type {
  TIssue,
  TIssueSubIssues,
  TIssueSubIssuesStateDistributionMap,
  TIssueSubIssuesIdMap,
  TPaginationData,
  TSubIssuesStateDistribution,
  TIssueServiceType,
  TLoader,
} from "@pi-dash/types";
// services
import { IssueService } from "@/services/issue";
// store
import type { IIssueDetail } from "./root.store";
import type { IWorkItemSubIssueFiltersStore } from "./sub_issues_filter.store";
import { WorkItemSubIssueFiltersStore } from "./sub_issues_filter.store";

export interface IIssueSubIssuesStoreActions {
  fetchSubIssues: (workspaceSlug: string, projectId: string, parentIssueId: string) => Promise<TIssueSubIssues>;
  fetchNextSubIssues: (
    workspaceSlug: string,
    projectId: string,
    parentIssueId: string
  ) => Promise<TIssueSubIssues | undefined>;
  createSubIssues: (
    workspaceSlug: string,
    projectId: string,
    parentIssueId: string,
    issueIds: string[]
  ) => Promise<void>;
  updateSubIssue: (
    workspaceSlug: string,
    projectId: string,
    parentIssueId: string,
    issueId: string,
    issueData: Partial<TIssue>,
    oldIssue?: Partial<TIssue>,
    fromModal?: boolean
  ) => Promise<void>;
  removeSubIssue: (workspaceSlug: string, projectId: string, parentIssueId: string, issueId: string) => Promise<void>;
  deleteSubIssue: (workspaceSlug: string, projectId: string, parentIssueId: string, issueId: string) => Promise<void>;
}

type TSubIssueHelpersKeys = "issue_visibility" | "preview_loader" | "issue_loader";
type TSubIssueHelpers = Record<TSubIssueHelpersKeys, string[]>;
export type TSubIssuePagination = Partial<TPaginationData> & { totalCount?: number };
export interface IIssueSubIssuesStore extends IIssueSubIssuesStoreActions {
  // observables
  subIssuesStateDistribution: TIssueSubIssuesStateDistributionMap;
  subIssues: TIssueSubIssuesIdMap;
  subIssuesPagination: Record<string, TSubIssuePagination>; // parent_issue_id -> pagination
  subIssueHelpers: Record<string, TSubIssueHelpers>; // parent_issue_id -> TSubIssueHelpers
  loader: TLoader;
  filters: IWorkItemSubIssueFiltersStore;
  // helper methods
  stateDistributionByIssueId: (issueId: string) => TSubIssuesStateDistribution | undefined;
  subIssuesByIssueId: (issueId: string) => string[] | undefined;
  subIssuePaginationByIssueId: (issueId: string) => TSubIssuePagination | undefined;
  subIssueHelpersByIssueId: (issueId: string) => TSubIssueHelpers;
  // actions
  fetchOtherProjectProperties: (workspaceSlug: string, projectIds: string[]) => Promise<void>;
  setSubIssueHelpers: (parentIssueId: string, key: TSubIssueHelpersKeys, value: string) => void;
}

// A generous first page so the common (small) parent loads in one request;
// paging only kicks in beyond it. Mirrored by the endpoint's default per_page.
export const SUB_ISSUES_PER_PAGE = 50;

export class IssueSubIssuesStore implements IIssueSubIssuesStore {
  // observables
  subIssuesStateDistribution: TIssueSubIssuesStateDistributionMap = {};
  subIssues: TIssueSubIssuesIdMap = {};
  subIssuesPagination: Record<string, TSubIssuePagination> = {};
  subIssueHelpers: Record<string, TSubIssueHelpers> = {};
  loader: TLoader = undefined;

  filters: IWorkItemSubIssueFiltersStore;
  // root store
  rootIssueDetailStore: IIssueDetail;
  // services
  serviceType;
  issueService;

  constructor(rootStore: IIssueDetail, serviceType: TIssueServiceType) {
    makeObservable(this, {
      // observables
      subIssuesStateDistribution: observable,
      subIssues: observable,
      subIssuesPagination: observable,
      subIssueHelpers: observable,
      loader: observable.ref,
      // actions
      setSubIssueHelpers: action,
      fetchSubIssues: action,
      fetchNextSubIssues: action,
      createSubIssues: action,
      updateSubIssue: action,
      removeSubIssue: action,
      deleteSubIssue: action,
      fetchOtherProjectProperties: action,
    });
    this.filters = new WorkItemSubIssueFiltersStore(this);
    // root store
    this.rootIssueDetailStore = rootStore;
    // services
    this.serviceType = serviceType;
    this.issueService = new IssueService(serviceType);
  }

  // helper methods
  stateDistributionByIssueId = (issueId: string) => {
    if (!issueId) return undefined;
    return this.subIssuesStateDistribution[issueId] ?? undefined;
  };

  subIssuesByIssueId = computedFn((issueId: string) => this.subIssues[issueId]);

  subIssuePaginationByIssueId = computedFn((issueId: string) => this.subIssuesPagination[issueId]);

  subIssueHelpersByIssueId = (issueId: string) => ({
    preview_loader: this.subIssueHelpers?.[issueId]?.preview_loader || [],
    issue_visibility: this.subIssueHelpers?.[issueId]?.issue_visibility || [],
    issue_loader: this.subIssueHelpers?.[issueId]?.issue_loader || [],
  });

  // keep the parent's sub_issues_count in sync with local mutations; the loaded
  // list may be a page, so prefer the paginated total over the list length
  adjustSubIssuesCount = (parentIssueId: string, delta: number) => {
    const pagination = this.subIssuesPagination[parentIssueId];
    const nextCount =
      pagination?.totalCount !== undefined
        ? Math.max(pagination.totalCount + delta, 0)
        : (this.subIssues[parentIssueId]?.length ?? 0);
    if (pagination?.totalCount !== undefined) set(this.subIssuesPagination, [parentIssueId, "totalCount"], nextCount);
    set(this.rootIssueDetailStore.rootIssueStore.issues.issuesMap, [parentIssueId, "sub_issues_count"], nextCount);
  };

  // actions
  setSubIssueHelpers = (parentIssueId: string, key: TSubIssueHelpersKeys, value: string) => {
    if (!parentIssueId || !key || !value) return;

    update(this.subIssueHelpers, [parentIssueId, key], (_subIssueHelpers: string[] = []) => {
      if (_subIssueHelpers.includes(value)) return pull(_subIssueHelpers, value);
      return concat(_subIssueHelpers, value);
    });
  };

  fetchSubIssues = async (workspaceSlug: string, projectId: string, parentIssueId: string) => {
    this.loader = "init-loader";
    const response = await this.issueService.subIssues(workspaceSlug, projectId, parentIssueId, {
      per_page: `${SUB_ISSUES_PER_PAGE}`,
    });

    const subIssuesStateDistribution = response?.state_distribution ?? {};

    const issueList = (response.sub_issues ?? []) as TIssue[];

    this.rootIssueDetailStore.rootIssueStore.issues.addIssue(issueList);

    // fetch other issues states and members when sub-issues are from different project
    if (issueList && issueList.length > 0) {
      const otherProjectIds = uniq(
        issueList.map((issue) => issue.project_id).filter((id) => !!id && id !== projectId)
      ) as string[];
      this.fetchOtherProjectProperties(workspaceSlug, otherProjectIds);
    }
    if (issueList) {
      this.rootIssueDetailStore.rootIssueStore.issues.updateIssue(parentIssueId, {
        sub_issues_count: response.total_count ?? issueList.length,
      });
    }

    runInAction(() => {
      set(this.subIssuesStateDistribution, parentIssueId, subIssuesStateDistribution);
      set(
        this.subIssues,
        parentIssueId,
        issueList.map((issue) => issue.id)
      );
      set(this.subIssuesPagination, parentIssueId, {
        nextCursor: response.next_cursor,
        prevCursor: response.prev_cursor,
        nextPageResults: response.next_page_results,
        totalCount: response.total_count,
      });
    });

    this.loader = undefined;
    return response;
  };

  fetchNextSubIssues = async (workspaceSlug: string, projectId: string, parentIssueId: string) => {
    const pagination = this.subIssuesPagination[parentIssueId];
    if (!pagination?.nextPageResults || !pagination?.nextCursor) return;

    this.loader = "pagination";
    try {
      const response = await this.issueService.subIssues(workspaceSlug, projectId, parentIssueId, {
        per_page: `${SUB_ISSUES_PER_PAGE}`,
        cursor: pagination.nextCursor,
      });

      const issueList = (response.sub_issues ?? []) as TIssue[];

      this.rootIssueDetailStore.rootIssueStore.issues.addIssue(issueList);

      // fetch other issues states and members when sub-issues are from different project
      if (issueList && issueList.length > 0) {
        const otherProjectIds = uniq(
          issueList.map((issue) => issue.project_id).filter((id) => !!id && id !== projectId)
        ) as string[];
        this.fetchOtherProjectProperties(workspaceSlug, otherProjectIds);
      }

      runInAction(() => {
        set(this.subIssuesStateDistribution, parentIssueId, response?.state_distribution ?? {});
        update(this.subIssues, [parentIssueId], (existingIds: string[] = []) =>
          uniq(
            concat(
              existingIds,
              issueList.map((issue) => issue.id)
            )
          )
        );
        set(this.subIssuesPagination, parentIssueId, {
          nextCursor: response.next_cursor,
          prevCursor: response.prev_cursor,
          nextPageResults: response.next_page_results,
          totalCount: response.total_count,
        });
      });

      this.loader = undefined;
      return response;
    } catch (error) {
      this.loader = undefined;
      throw error;
    }
  };

  createSubIssues = async (workspaceSlug: string, projectId: string, parentIssueId: string, issueIds: string[]) => {
    const response = await this.issueService.addSubIssues(workspaceSlug, projectId, parentIssueId, {
      sub_issue_ids: issueIds,
    });

    const subIssuesStateDistribution = response?.state_distribution;
    const subIssues = response.sub_issues as TIssue[];

    // fetch other issues states and members when sub-issues are from different project
    if (subIssues && subIssues.length > 0) {
      const otherProjectIds = uniq(
        subIssues.map((issue) => issue.project_id).filter((id) => !!id && id !== projectId)
      ) as string[];
      this.fetchOtherProjectProperties(workspaceSlug, otherProjectIds);
    }

    runInAction(() => {
      Object.keys(subIssuesStateDistribution).forEach((key) => {
        const stateGroup = key as keyof TSubIssuesStateDistribution;
        update(this.subIssuesStateDistribution, [parentIssueId, stateGroup], (stateDistribution) => {
          if (!stateDistribution) return subIssuesStateDistribution[stateGroup];
          return concat(stateDistribution, subIssuesStateDistribution[stateGroup]);
        });
      });

      const newSubIssueIds = subIssues.map((issue) => issue.id);
      update(this.subIssues, [parentIssueId], (issues) => {
        if (!issues) return newSubIssueIds;
        return concat(issues, newSubIssueIds);
      });
    });

    this.rootIssueDetailStore.rootIssueStore.issues.addIssue(subIssues);

    // update sub-issues_count of the parent issue
    runInAction(() => {
      this.adjustSubIssuesCount(parentIssueId, subIssues.length);
    });

    return;
  };

  updateSubIssue = async (
    workspaceSlug: string,
    projectId: string,
    parentIssueId: string,
    issueId: string,
    issueData: Partial<TIssue>,
    oldIssue: Partial<TIssue> = {},
    fromModal: boolean = false
  ) => {
    if (!fromModal)
      await this.rootIssueDetailStore.rootIssueStore.projectIssues.updateIssue(
        workspaceSlug,
        projectId,
        issueId,
        issueData
      );

    // parent update
    if (issueData.hasOwnProperty("parent_id") && issueData.parent_id !== oldIssue.parent_id) {
      runInAction(() => {
        if (oldIssue.parent_id) pull(this.subIssues[oldIssue.parent_id], issueId);
        if (issueData.parent_id)
          set(this.subIssues, [issueData.parent_id], concat(this.subIssues[issueData.parent_id], issueId));
      });
    }

    // state update
    if (issueData.hasOwnProperty("state_id") && issueData.state_id !== oldIssue.state_id) {
      let oldIssueStateGroup: string | undefined = undefined;
      let issueStateGroup: string | undefined = undefined;

      if (oldIssue.state_id) {
        const state = this.rootIssueDetailStore.rootIssueStore.rootStore.state.getStateById(oldIssue.state_id);
        if (state?.group) oldIssueStateGroup = state.group;
      }

      if (issueData.state_id) {
        const state = this.rootIssueDetailStore.rootIssueStore.rootStore.state.getStateById(issueData.state_id);
        if (state?.group) issueStateGroup = state.group;
      }

      if (oldIssueStateGroup && issueStateGroup && issueStateGroup !== oldIssueStateGroup) {
        runInAction(() => {
          if (oldIssueStateGroup)
            update(this.subIssuesStateDistribution, [parentIssueId, oldIssueStateGroup], (stateDistribution) => {
              if (!stateDistribution) return;
              return pull(stateDistribution, issueId);
            });

          if (issueStateGroup)
            update(this.subIssuesStateDistribution, [parentIssueId, issueStateGroup], (stateDistribution) => {
              if (!stateDistribution) return [issueId];
              return concat(stateDistribution, issueId);
            });
        });
      }
    }

    return;
  };

  removeSubIssue = async (workspaceSlug: string, projectId: string, parentIssueId: string, issueId: string) => {
    await this.rootIssueDetailStore.rootIssueStore.projectIssues.updateIssue(workspaceSlug, projectId, issueId, {
      parent_id: null,
    });

    const issue = this.rootIssueDetailStore.issue.getIssueById(issueId);
    if (issue && issue.state_id) {
      let issueStateGroup: string | undefined = undefined;
      const state = this.rootIssueDetailStore.rootIssueStore.rootStore.state.getStateById(issue.state_id);
      if (state?.group) issueStateGroup = state.group;

      if (issueStateGroup) {
        runInAction(() => {
          if (issueStateGroup)
            update(this.subIssuesStateDistribution, [parentIssueId, issueStateGroup], (stateDistribution) => {
              if (!stateDistribution) return;
              return pull(stateDistribution, issueId);
            });
        });
      }
    }

    runInAction(() => {
      pull(this.subIssues[parentIssueId], issueId);
      // update sub-issues_count of the parent issue
      this.adjustSubIssuesCount(parentIssueId, -1);
    });

    return;
  };

  deleteSubIssue = async (workspaceSlug: string, projectId: string, parentIssueId: string, issueId: string) => {
    await this.rootIssueDetailStore.rootIssueStore.projectIssues.removeIssue(workspaceSlug, projectId, issueId);

    const issue = this.rootIssueDetailStore.issue.getIssueById(issueId);
    if (issue && issue.state_id) {
      let issueStateGroup: string | undefined = undefined;
      const state = this.rootIssueDetailStore.rootIssueStore.rootStore.state.getStateById(issue.state_id);
      if (state?.group) issueStateGroup = state.group;

      if (issueStateGroup) {
        runInAction(() => {
          if (issueStateGroup)
            update(this.subIssuesStateDistribution, [parentIssueId, issueStateGroup], (stateDistribution) => {
              if (!stateDistribution) return;
              return pull(stateDistribution, issueId);
            });
        });
      }
    }

    runInAction(() => {
      pull(this.subIssues[parentIssueId], issueId);
      // update sub-issues_count of the parent issue
      this.adjustSubIssuesCount(parentIssueId, -1);
    });

    return;
  };

  fetchOtherProjectProperties = async (workspaceSlug: string, projectIds: string[]) => {
    if (projectIds.length > 0) {
      for (const projectId of projectIds) {
        // fetching other project states
        this.rootIssueDetailStore.rootIssueStore.rootStore.state.fetchProjectStates(workspaceSlug, projectId);
        // fetching other project members
        this.rootIssueDetailStore.rootIssueStore.rootStore.memberRoot.project.fetchProjectMembers(
          workspaceSlug,
          projectId
        );
        // fetching other project labels
        this.rootIssueDetailStore.rootIssueStore.rootStore.label.fetchProjectLabels(workspaceSlug, projectId);
        // fetching other project cycles
        this.rootIssueDetailStore.rootIssueStore.rootStore.cycle.fetchAllCycles(workspaceSlug, projectId);
        // fetching other project modules
        this.rootIssueDetailStore.rootIssueStore.rootStore.module.fetchModules(workspaceSlug, projectId);
        // fetching other project estimates
        this.rootIssueDetailStore.rootIssueStore.rootStore.projectEstimate.getProjectEstimates(
          workspaceSlug,
          projectId
        );
      }
    }
  };
}
