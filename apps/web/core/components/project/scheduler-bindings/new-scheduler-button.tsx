/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { observer } from "mobx-react";
import useSWR, { useSWRConfig } from "swr";
// pi dash imports
import { EUserPermissions, EUserPermissionsLevel } from "@pi-dash/constants";
import { useTranslation } from "@pi-dash/i18n";
import type { IScheduler, ISchedulerBinding } from "@pi-dash/services";
import { SchedulerService } from "@pi-dash/services";
import { Button } from "@pi-dash/ui";
// components
import { NewSchedulerModal } from "@/components/project/scheduler-bindings/new-scheduler-modal";
// hooks
import { useUserPermissions } from "@/hooks/store/user";

const schedulerService = new SchedulerService();

type Props = {
  workspaceSlug: string;
  projectId: string;
};

/**
 * True for every SWR key that holds this project's scheduler bindings or
 * calendar occurrences (any date window), so one install refreshes both the
 * List and Calendar tabs.
 */
export function isProjectSchedulerKey(key: unknown, workspaceSlug: string, projectId: string): boolean {
  if (!Array.isArray(key)) return false;
  const [kind, slug, project] = key as unknown[];
  return (
    (kind === "scheduler-bindings" || kind === "scheduler-occurrences") &&
    slug === workspaceSlug &&
    project === projectId
  );
}

/**
 * "New Scheduler" control for the project Schedulers tab bar. Rendered by
 * the shared schedulers layout, so it shows on the List and Calendar tabs
 * (and binding detail pages). Project admins only; members see nothing.
 */
export const NewSchedulerButton = observer(function NewSchedulerButton(props: Props) {
  const { workspaceSlug, projectId } = props;
  const { t } = useTranslation();
  const { allowPermissions } = useUserPermissions();
  const { mutate: mutateCache } = useSWRConfig();
  const [open, setOpen] = useState(false);

  const canManage = allowPermissions([EUserPermissions.ADMIN], EUserPermissionsLevel.PROJECT, workspaceSlug, projectId);
  // Definition CRUD is workspace-admin territory; gates the modal's "Create new" path.
  const canCreateScheduler = allowPermissions([EUserPermissions.ADMIN], EUserPermissionsLevel.WORKSPACE, workspaceSlug);

  // Same SWR keys as the List panel and the Calendar, so the data is shared.
  const { data: bindings } = useSWR<ISchedulerBinding[]>(
    canManage && workspaceSlug && projectId ? ["scheduler-bindings", workspaceSlug, projectId] : null,
    () => schedulerService.listBindings(workspaceSlug, projectId)
  );
  const { data: schedulers, mutate: mutateSchedulers } = useSWR<IScheduler[]>(
    canManage && workspaceSlug ? ["schedulers", workspaceSlug] : null,
    () => schedulerService.listSchedulers(workspaceSlug)
  );

  if (!canManage) return null;

  return (
    <>
      <Button size="sm" onClick={() => setOpen(true)}>
        {t("New Scheduler")}
      </Button>
      <NewSchedulerModal
        isOpen={open}
        onClose={() => setOpen(false)}
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        availableSchedulers={schedulers ?? []}
        existingBindings={bindings ?? []}
        canCreateScheduler={canCreateScheduler}
        onInstalled={() => void mutateCache((key) => isProjectSchedulerKey(key, workspaceSlug, projectId))}
        onSchedulersChanged={() => void mutateSchedulers()}
      />
    </>
  );
});
