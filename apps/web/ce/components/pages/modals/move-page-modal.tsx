/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useMemo, useState } from "react";
import { observer } from "mobx-react";
import { useParams } from "next/navigation";
// pi dash imports
import { EUserPermissions } from "@pi-dash/constants";
import { useTranslation } from "@pi-dash/i18n";
import { Button } from "@pi-dash/propel/button";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import { EModalPosition, EModalWidth, ModalCore } from "@pi-dash/ui";
import { getPageName } from "@pi-dash/utils";
// components
import { ProjectDropdown } from "@/components/dropdowns/project/dropdown";
// hooks
import { useProject } from "@/hooks/store/use-project";
import { useUserPermissions } from "@/hooks/store/user";
import { useAppRouter } from "@/hooks/use-app-router";
// pi dash web hooks
import { EPageStoreType, usePageStore } from "@/pi-dash-web/hooks/store";
// store types
import type { TPageInstance } from "@/store/pages/base-page";

export type TMovePageModalProps = {
  isOpen: boolean;
  onClose: () => void;
  page: TPageInstance;
  storeType?: EPageStoreType;
};

/** Roles that can write to a project, i.e. receive a moved page. */
const ROLES_ALLOWED_AS_MOVE_TARGET = new Set<number>([EUserPermissions.ADMIN, EUserPermissions.MEMBER]);

/**
 * Move a page, with its sub-pages, to another project of the workspace. Only projects the
 * user can write to are offered (joined, not archived, admin or member); the server re-checks
 * every rule and its `{ error }` message is shown as-is when it refuses.
 */
export const MovePageModal = observer(function MovePageModal(props: TMovePageModalProps) {
  const { isOpen, onClose, page, storeType = EPageStoreType.PROJECT } = props;
  // states
  const [targetProjectId, setTargetProjectId] = useState<string | null>(null);
  const [isMoving, setIsMoving] = useState(false);
  // params
  const { workspaceSlug, projectId: routeProjectId, pageId: routePageId } = useParams();
  // router
  const router = useAppRouter();
  // store hooks
  const { t } = useTranslation();
  const { joinedProjectIds, getProjectById } = useProject();
  const { getProjectRoleByWorkspaceSlugAndProjectId } = useUserPermissions();
  const { movePage } = usePageStore(storeType);
  // derived values
  const { id: pageId, name, project_ids } = page;
  const sourceProjectId = routeProjectId?.toString() ?? project_ids?.[0];
  const canMoveToProject = (projectId: string) => {
    if (projectId === sourceProjectId) return false;
    const role = getProjectRoleByWorkspaceSlugAndProjectId(workspaceSlug?.toString() ?? "", projectId);
    return !!role && ROLES_ALLOWED_AS_MOVE_TARGET.has(role);
  };
  const eligibleProjectIds = useMemo(
    () => joinedProjectIds.filter((projectId) => canMoveToProject(projectId)),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [joinedProjectIds, sourceProjectId, workspaceSlug, getProjectRoleByWorkspaceSlugAndProjectId]
  );

  const handleClose = () => {
    if (isMoving) return;
    setTargetProjectId(null);
    onClose();
  };

  const handleMove = async () => {
    if (!workspaceSlug || !sourceProjectId || !pageId || !targetProjectId) return;
    setIsMoving(true);
    try {
      await movePage(workspaceSlug.toString(), sourceProjectId, pageId, targetProjectId);
      setToast({
        type: TOAST_TYPE.SUCCESS,
        title: t("Success!"),
        message: t("Page moved to {project}.", { project: getProjectById(targetProjectId)?.name ?? "" }),
      });
      setIsMoving(false);
      setTargetProjectId(null);
      onClose();
      // the page keeps its id, so its detail route simply changes project
      if (routePageId?.toString() === pageId) {
        router.push(`/${workspaceSlug.toString()}/projects/${targetProjectId}/pages/${pageId}/`);
      }
    } catch (error) {
      const serverMessage = (error as { error?: unknown } | undefined)?.error;
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Error!"),
        message:
          typeof serverMessage === "string" && serverMessage
            ? serverMessage
            : t("Page could not be moved. Please try again."),
      });
      setIsMoving(false);
    }
  };

  if (!page || !pageId) return null;

  const noEligibleProjects = eligibleProjectIds.length === 0;

  return (
    <ModalCore isOpen={isOpen} handleClose={handleClose} position={EModalPosition.CENTER} width={EModalWidth.XL}>
      <div className="flex flex-col gap-5 p-5">
        <div className="flex flex-col gap-1">
          <h3 className="text-18 font-medium text-primary">{t("Move to project")}</h3>
          <p className="text-13 text-secondary">
            {t("Move {page} and its sub-pages to another project in this workspace.", {
              page: getPageName(name),
            })}
          </p>
        </div>
        {noEligibleProjects ? (
          <p className="rounded-md border border-subtle px-3 py-6 text-center text-13 text-secondary">
            {t("There is no other project you can move this page to.")}
          </p>
        ) : (
          <div className="h-8">
            <ProjectDropdown
              value={targetProjectId}
              onChange={setTargetProjectId}
              multiple={false}
              buttonVariant="border-with-text"
              placeholder={t("Select a project")}
              renderCondition={(projectId) => eligibleProjectIds.includes(projectId)}
              disabled={isMoving}
            />
          </div>
        )}
        <div className="flex justify-end gap-2">
          <Button variant="secondary" onClick={handleClose} disabled={isMoving}>
            {t("Cancel")}
          </Button>
          <Button onClick={handleMove} loading={isMoving} disabled={isMoving || !targetProjectId}>
            {isMoving ? t("Moving") : t("Move")}
          </Button>
        </div>
      </div>
    </ModalCore>
  );
});
