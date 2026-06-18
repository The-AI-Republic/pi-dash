/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { observer } from "mobx-react";
import { ExternalLink, GitMerge, GitPullRequest, GitPullRequestDraft } from "lucide-react";
import useSWR from "swr";
// pi dash imports
import { Badge } from "@pi-dash/propel/badge";
import type { TBadgeVariant } from "@pi-dash/propel/badge";
import { TrashIcon } from "@pi-dash/propel/icons";
import { Input } from "@pi-dash/propel/input";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import { Tooltip } from "@pi-dash/propel/tooltip";
import type { IGithubPullRequestLink } from "@pi-dash/types";
import { Collapsible } from "@pi-dash/ui";
// constants
import { GITHUB_ISSUE_PULL_REQUESTS } from "@/constants/fetch-keys";
// services
import { GithubIntegrationService } from "@/services/integrations/github.service";

const githubService = new GithubIntegrationService();

type Props = {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  disabled?: boolean;
};

type TBadge = {
  label: string;
  variant: TBadgeVariant;
  icon: typeof GitPullRequest;
};

const getPullRequestBadge = (link: IGithubPullRequestLink): TBadge => {
  if (link.merged) return { label: "Merged", variant: "brand", icon: GitMerge };
  if (link.draft) return { label: "Draft", variant: "neutral", icon: GitPullRequestDraft };
  if (link.state === "open") return { label: "Open", variant: "success", icon: GitPullRequest };
  return { label: "Closed", variant: "danger", icon: GitPullRequest };
};

const toErrorMessage = (error: unknown, fallback: string): string => {
  const data = error as { error?: string; detail?: string } | null;
  return data?.error ?? data?.detail ?? fallback;
};

export const IssueGithubPullRequestsRoot = observer(function IssueGithubPullRequestsRoot(props: Props) {
  const { workspaceSlug, projectId, issueId, disabled = false } = props;
  // state
  const [isOpen, setIsOpen] = useState(true);
  const [url, setUrl] = useState("");
  const [isSubmitting, setIsSubmitting] = useState(false);
  // fetching
  const shouldFetch = Boolean(workspaceSlug && projectId && issueId);
  const { data: pullRequests, mutate: mutatePullRequests } = useSWR(
    shouldFetch ? GITHUB_ISSUE_PULL_REQUESTS(issueId) : null,
    shouldFetch ? () => githubService.listIssuePullRequests(workspaceSlug, projectId, issueId) : null
  );

  const handleAttach = async () => {
    const trimmedUrl = url.trim();
    if (!trimmedUrl || isSubmitting) return;
    setIsSubmitting(true);
    try {
      await githubService.attachIssuePullRequest(workspaceSlug, projectId, issueId, { url: trimmedUrl });
      setUrl("");
      await mutatePullRequests();
    } catch (error: unknown) {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: "Pull request not attached",
        message: toErrorMessage(error, "The pull request could not be attached."),
      });
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleDetach = async (linkId: string) => {
    try {
      await githubService.detachIssuePullRequest(workspaceSlug, projectId, issueId, linkId);
      await mutatePullRequests();
    } catch (error: unknown) {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: "Pull request not detached",
        message: toErrorMessage(error, "The pull request could not be detached."),
      });
    }
  };

  // Gate: keep issue detail uncluttered — only surface the section once at least
  // one PR is linked (the coding agent attaches the first one via the CLI). The
  // attach/detach controls then live alongside the existing links.
  if (!pullRequests || pullRequests.length === 0) return null;

  return (
    <Collapsible
      isOpen={isOpen}
      onToggle={() => setIsOpen((prev) => !prev)}
      buttonClassName="w-full"
      title={
        <div className="flex items-center gap-1 py-1 text-13 font-medium text-primary">
          <span>Pull requests</span>
          <span className="text-tertiary">{pullRequests.length}</span>
        </div>
      }
    >
      <div className="mt-1 flex flex-col gap-1">
        {pullRequests.map((link) => {
          const badge = getPullRequestBadge(link);
          const BadgeIcon = badge.icon;
          return (
            <div
              key={link.id}
              className="group flex items-center gap-2 rounded-md px-2 py-1.5 duration-300 hover:bg-surface-2"
            >
              <Badge variant={badge.variant} size="sm" prependIcon={<BadgeIcon />}>
                {badge.label}
              </Badge>
              <a
                href={link.url}
                target="_blank"
                rel="noopener noreferrer"
                className="flex min-w-0 flex-1 items-center gap-1 truncate text-secondary hover:text-primary"
              >
                <span className="truncate">
                  {link.title || `${link.repo_owner}/${link.repo_name} #${link.pr_number}`}
                </span>
                <ExternalLink className="h-3 w-3 shrink-0" />
              </a>
              {!disabled && (
                <Tooltip tooltipContent="Detach pull request">
                  <button
                    type="button"
                    className="hover:bg-surface-3 grid h-6 w-6 shrink-0 place-items-center rounded-sm text-tertiary opacity-0 duration-300 outline-none group-hover:opacity-100 hover:text-danger-primary"
                    onClick={() => handleDetach(link.id)}
                  >
                    <TrashIcon className="h-3.5 w-3.5" />
                  </button>
                </Tooltip>
              )}
            </div>
          );
        })}

        {!disabled && (
          <form
            className="mt-1 flex items-center gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              void handleAttach();
            }}
          >
            <Input
              type="url"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              placeholder="Paste a GitHub pull request URL"
              className="w-full text-11"
              inputSize="xs"
              disabled={isSubmitting}
            />
            <button
              type="submit"
              className="text-on-accent shrink-0 rounded-md bg-accent-primary px-2.5 py-1 font-medium duration-300 outline-none hover:bg-accent-primary-hover disabled:cursor-not-allowed disabled:opacity-60"
              disabled={isSubmitting || !url.trim()}
            >
              Attach PR
            </button>
          </form>
        )}
      </div>
    </Collapsible>
  );
});
