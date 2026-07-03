/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useMemo, useState } from "react";
import { observer } from "mobx-react";
import { useParams, useRouter } from "next/navigation";
import { AlertCircle } from "lucide-react";
// pi dash imports
import { useTranslation } from "@pi-dash/i18n";
import { Logo } from "@pi-dash/propel/emoji-icon-picker";
import { SearchIcon, CloseIcon } from "@pi-dash/propel/icons";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import { EIssuesStoreType } from "@pi-dash/types";
import { EModalPosition, EModalWidth, ModalCore } from "@pi-dash/ui";
import { generateWorkItemLink } from "@pi-dash/utils";
// hooks
import { useIssues } from "@/hooks/store/use-issues";
import { useProject } from "@/hooks/store/use-project";

type Props = {
  isOpen: boolean;
  onClose: () => void;
  issueId: string;
  projectId: string;
};

export const MoveIssueModal = observer(function MoveIssueModal(props: Props) {
  const { isOpen, onClose, issueId, projectId } = props;
  // states
  const [query, setQuery] = useState("");
  const [movingToProjectId, setMovingToProjectId] = useState<string | null>(null);
  // router
  const router = useRouter();
  const { workspaceSlug } = useParams();
  // pi dash hooks
  const { t } = useTranslation();
  // store hooks
  const { joinedProjectIds, getProjectById, getProjectIdentifierById, fetchProjects } = useProject();
  const {
    issues: { moveIssue },
  } = useIssues(EIssuesStoreType.PROJECT);

  // Only same-workspace projects the user belongs to, excluding the current one.
  // Cross-workspace moves are intentionally unsupported.
  const targetProjectIds = useMemo(
    () => joinedProjectIds.filter((id) => id !== projectId),
    [joinedProjectIds, projectId]
  );

  useEffect(() => {
    if (!isOpen || !workspaceSlug) return;

    fetchProjects(workspaceSlug.toString()).catch(() => {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Error!"),
        message: t("Projects could not be loaded. Please try again."),
      });
    });
  }, [fetchProjects, isOpen, t, workspaceSlug]);

  const filteredProjectIds = targetProjectIds.filter((id) => {
    const project = getProjectById(id);
    // Drop ids whose project details aren't loaded — otherwise an unresolved
    // project matches every query and renders as `null` below, suppressing the
    // "no other projects" empty state and leaving a blank area.
    if (!project) return false;
    const projectQuery = `${project.identifier ?? ""} ${project.name ?? ""}`.toLowerCase();
    return projectQuery.includes(query.toLowerCase());
  });

  const handleClose = () => {
    setQuery("");
    setMovingToProjectId(null);
    onClose();
  };

  const handleMove = async (targetProjectId: string) => {
    if (!workspaceSlug || movingToProjectId) return;
    setMovingToProjectId(targetProjectId);
    try {
      const movedIssue = await moveIssue(workspaceSlug.toString(), projectId, issueId, targetProjectId);
      setToast({
        type: TOAST_TYPE.SUCCESS,
        title: t("Success!"),
        message: t("Work item moved successfully."),
      });
      handleClose();
      // The work item now lives in the target project — its old URL is stale,
      // so send the user to it in its new home.
      const link = generateWorkItemLink({
        workspaceSlug: workspaceSlug.toString(),
        projectId: targetProjectId,
        issueId,
        projectIdentifier: getProjectIdentifierById(targetProjectId),
        sequenceId: movedIssue?.sequence_id,
      });
      router.push(link);
    } catch (error) {
      const message =
        (error as { error?: string; detail?: string })?.error ??
        (error as { detail?: string })?.detail ??
        t("Work item could not be moved. Please try again.");
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Error!"),
        message,
      });
      setMovingToProjectId(null);
    }
  };

  return (
    <ModalCore isOpen={isOpen} handleClose={handleClose} position={EModalPosition.TOP} width={EModalWidth.XXL}>
      <div className="flex flex-col gap-4 py-5">
        <div className="flex items-center justify-between px-5">
          <h4 className="text-18 font-medium text-primary">{t("Move to project")}</h4>
          <button onClick={handleClose}>
            <CloseIcon className="h-4 w-4" />
          </button>
        </div>
        <div className="flex items-center gap-2 border-b border-subtle px-5 pb-3">
          <SearchIcon className="h-4 w-4 text-secondary" />
          <input
            className="w-full bg-transparent text-13 outline-none"
            placeholder={t("Search for a project...")}
            onChange={(e) => setQuery(e.target.value)}
            value={query}
          />
        </div>
        <div className="flex max-h-80 w-full flex-col items-start gap-2 overflow-y-auto px-5">
          {filteredProjectIds.length > 0 ? (
            filteredProjectIds.map((optionId) => {
              const projectDetails = getProjectById(optionId);
              if (!projectDetails) return null;

              return (
                <button
                  key={optionId}
                  disabled={!!movingToProjectId}
                  className="flex w-full items-center gap-3 rounded-sm px-4 py-3 text-13 text-secondary hover:bg-surface-2 disabled:cursor-not-allowed disabled:opacity-60"
                  onClick={() => handleMove(optionId)}
                >
                  <span className="flex h-5 w-5 flex-shrink-0 items-center justify-center">
                    <Logo logo={projectDetails.logo_props} size={16} />
                  </span>
                  <div className="flex w-full items-center justify-between gap-2 truncate">
                    <span className="truncate">{projectDetails.name}</span>
                    <span className="flex flex-shrink-0 items-center rounded-full bg-layer-1 px-2 text-tertiary">
                      {projectDetails.identifier}
                    </span>
                  </div>
                </button>
              );
            })
          ) : (
            <div className="flex w-full items-center justify-center gap-4 p-5 text-13">
              <AlertCircle className="h-3.5 w-3.5 text-secondary" />
              <span className="text-center text-secondary">
                {t("No other projects available to move this work item to.")}
              </span>
            </div>
          )}
        </div>
      </div>
    </ModalCore>
  );
});
