/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useRef } from "react";
import { observer } from "mobx-react";
import Link from "next/link";
import { MoveDiagonal, MoveRight } from "lucide-react";
// pi dash imports
import { useTranslation } from "@pi-dash/i18n";
import { CenterPanelIcon, CopyLinkIcon, FullScreenPanelIcon, SidePanelIcon } from "@pi-dash/propel/icons";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import { Tooltip } from "@pi-dash/propel/tooltip";
import type { TNameDescriptionLoader } from "@pi-dash/types";
import { EIssuesStoreType } from "@pi-dash/types";
import { CustomSelect } from "@pi-dash/ui";
import { copyUrlToClipboard, generateWorkItemLink } from "@pi-dash/utils";
// hooks
import { useIssueDetail } from "@/hooks/store/use-issue-detail";
import { useIssues } from "@/hooks/store/use-issues";
import { useProject } from "@/hooks/store/use-project";
import { useUser } from "@/hooks/store/user";
import { usePlatformOS } from "@/hooks/use-platform-os";
// local imports
import { IssueSubscription } from "../issue-detail/subscription";
import { WorkItemDetailQuickActions } from "../issue-layouts/quick-action-dropdowns";
import { NameDescriptionUpdateStatus } from "../issue-update-status";
import { IconButton } from "@pi-dash/propel/icon-button";

export type TPeekModes = "side-peek" | "modal" | "full-screen";

const PEEK_OPTIONS: { key: TPeekModes; icon: any; i18n_title: string }[] = [
  {
    key: "side-peek",
    icon: SidePanelIcon,
    i18n_title: "Side Peek",
  },
  {
    key: "modal",
    icon: CenterPanelIcon,
    i18n_title: "Modal",
  },
  {
    key: "full-screen",
    icon: FullScreenPanelIcon,
    i18n_title: "Full Screen",
  },
];

export type PeekOverviewHeaderProps = {
  peekMode: TPeekModes;
  setPeekMode: (value: TPeekModes) => void;
  removeRoutePeekId: () => void;
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  isArchived: boolean;
  disabled: boolean;
  embedIssue: boolean;
  toggleDeleteIssueModal: (value: boolean) => void;
  toggleArchiveIssueModal: (value: boolean) => void;
  toggleDuplicateIssueModal: (value: boolean) => void;
  toggleEditIssueModal: (value: boolean) => void;
  toggleMoveIssueModal: (value: boolean) => void;
  handleRestoreIssue: () => Promise<void>;
  isSubmitting: TNameDescriptionLoader;
};

export const IssuePeekOverviewHeader = observer(function IssuePeekOverviewHeader(props: PeekOverviewHeaderProps) {
  const {
    peekMode,
    setPeekMode,
    workspaceSlug,
    projectId,
    issueId,
    isArchived,
    disabled,
    embedIssue = false,
    removeRoutePeekId,
    toggleDeleteIssueModal,
    toggleArchiveIssueModal,
    toggleDuplicateIssueModal,
    toggleEditIssueModal,
    toggleMoveIssueModal,
    handleRestoreIssue,
    isSubmitting,
  } = props;
  // ref
  const parentRef = useRef<HTMLDivElement>(null);
  const { t } = useTranslation();
  // store hooks
  const { data: currentUser } = useUser();
  const {
    issue: { getIssueById },
    setPeekIssue,
    removeIssue,
    archiveIssue,
    getIsIssuePeeked,
  } = useIssueDetail();
  const { isMobile } = usePlatformOS();
  const { getProjectIdentifierById } = useProject();
  // derived values
  const issueDetails = getIssueById(issueId);
  const currentMode = PEEK_OPTIONS.find((m) => m.key === peekMode);
  const projectIdentifier = getProjectIdentifierById(issueDetails?.project_id);
  const {
    issues: { removeIssue: removeArchivedIssue },
  } = useIssues(EIssuesStoreType.ARCHIVED);

  const workItemLink = generateWorkItemLink({
    workspaceSlug,
    projectId: issueDetails?.project_id,
    issueId,
    projectIdentifier,
    sequenceId: issueDetails?.sequence_id,
    isArchived,
  });

  const handleCopyText = async (e: React.MouseEvent<HTMLButtonElement>) => {
    e.stopPropagation();
    e.preventDefault();
    await copyUrlToClipboard(workItemLink);
    setToast({
      type: TOAST_TYPE.SUCCESS,
      title: t("Link copied!"),
      message: t("Link copied to clipboard"),
    });
  };

  const handleDeleteIssue = async () => {
    try {
      const deleteIssue = issueDetails?.archived_at ? removeArchivedIssue : removeIssue;

      await deleteIssue(workspaceSlug, projectId, issueId);
      setPeekIssue(undefined);
    } catch (_error) {
      setToast({
        title: t("Error!"),
        type: TOAST_TYPE.ERROR,
        message: t("{entity} delete failed", {
          entity: t("{count, plural, one {Work item} other {Work items}}", { count: 1 }),
        }),
      });
    }
  };

  const handleArchiveIssue = async () => {
    await archiveIssue(workspaceSlug, projectId, issueId);
    // check and remove if issue is peeked
    if (getIsIssuePeeked(issueId)) {
      removeRoutePeekId();
    }
  };

  return (
    <div
      className={`relative flex items-center justify-between p-4 ${
        currentMode?.key === "full-screen" ? "border-b border-subtle" : ""
      }`}
    >
      <div className="flex items-center gap-4">
        <Tooltip tooltipContent={t("Close the peek view")} isMobile={isMobile}>
          <button onClick={removeRoutePeekId}>
            <MoveRight className="h-4 w-4 text-tertiary hover:text-secondary" />
          </button>
        </Tooltip>

        <Tooltip tooltipContent={t("Open work item in full screen")} isMobile={isMobile}>
          <Link href={workItemLink} onClick={() => removeRoutePeekId()}>
            <MoveDiagonal className="h-4 w-4 text-tertiary hover:text-secondary" />
          </Link>
        </Tooltip>
        {currentMode && embedIssue === false && (
          <div className="flex flex-shrink-0 items-center gap-2">
            <CustomSelect
              value={currentMode}
              onChange={(val: any) => setPeekMode(val)}
              customButton={
                <Tooltip tooltipContent={t("Toggle peek view layout")} isMobile={isMobile}>
                  <button type="button" className="">
                    <currentMode.icon className="h-4 w-4 text-tertiary hover:text-secondary" />
                  </button>
                </Tooltip>
              }
            >
              {PEEK_OPTIONS.map((mode) => (
                <CustomSelect.Option key={mode.key} value={mode.key}>
                  <div
                    className={`flex items-center gap-1.5 ${
                      currentMode.key === mode.key ? "text-secondary" : "text-placeholder hover:text-secondary"
                    }`}
                  >
                    <mode.icon className="-my-1 h-4 w-4 flex-shrink-0" />
                    {t(mode.i18n_title)}
                  </div>
                </CustomSelect.Option>
              ))}
            </CustomSelect>
          </div>
        )}
      </div>
      <div className="flex items-center gap-x-4">
        <NameDescriptionUpdateStatus isSubmitting={isSubmitting} />
        <div className="flex items-center gap-2">
          {currentUser && !isArchived && (
            <IssueSubscription workspaceSlug={workspaceSlug} projectId={projectId} issueId={issueId} />
          )}
          <Tooltip tooltipContent={t("Copy link")} isMobile={isMobile}>
            <IconButton variant="secondary" size="lg" onClick={handleCopyText} icon={CopyLinkIcon} />
          </Tooltip>
          {issueDetails && (
            <WorkItemDetailQuickActions
              parentRef={parentRef}
              issue={issueDetails}
              handleDelete={handleDeleteIssue}
              handleArchive={handleArchiveIssue}
              handleRestore={handleRestoreIssue}
              readOnly={disabled}
              toggleDeleteIssueModal={toggleDeleteIssueModal}
              toggleArchiveIssueModal={toggleArchiveIssueModal}
              toggleDuplicateIssueModal={toggleDuplicateIssueModal}
              toggleEditIssueModal={toggleEditIssueModal}
              toggleMoveIssueModal={toggleMoveIssueModal}
              isPeekMode
            />
          )}
        </div>
      </div>
    </div>
  );
});
