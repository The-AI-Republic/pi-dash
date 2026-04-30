/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { observer } from "mobx-react";
import { useParams } from "react-router";
import useSWR from "swr";
// pi dash imports
import { EUserPermissions, EUserPermissionsLevel } from "@pi-dash/constants";
import { useTranslation } from "@pi-dash/i18n";
import type { IScheduler, ISchedulerBinding } from "@pi-dash/services";
import { SchedulerService } from "@pi-dash/services";
import { Badge, Button } from "@pi-dash/ui";
// components
import { NotAuthorizedView } from "@/components/auth-screens/not-authorized-view";
import { PageHead } from "@/components/core/page-title";
import { EditSchedulerBindingModal } from "@/components/project/scheduler-bindings/edit-binding-modal";
import { InstallSchedulerBindingModal } from "@/components/project/scheduler-bindings/install-binding-modal";
import { UninstallSchedulerBindingModal } from "@/components/project/scheduler-bindings/uninstall-binding-modal";
import { SettingsContentWrapper } from "@/components/settings/content-wrapper";
// hooks
import { useProject } from "@/hooks/store/use-project";
import { useUserPermissions } from "@/hooks/store/user";
import { SchedulersProjectSettingsHeader } from "./header";

const schedulerService = new SchedulerService();

const SchedulerBindingsSettingsPage = observer(function SchedulerBindingsSettingsPage() {
  const { workspaceSlug, projectId } = useParams<{ workspaceSlug: string; projectId: string }>();
  const { currentProjectDetails } = useProject();
  const { workspaceUserInfo, allowPermissions } = useUserPermissions();
  const { t } = useTranslation();

  const slug = workspaceSlug ?? "";
  const project = projectId ?? "";

  // Project admin only — matches the route's access list and the
  // backend's project-admin gate on binding mutations. Surfacing the
  // page to non-admins would only show a read-only list with no
  // actions, which adds no value over the workspace catalog view.
  const canManage = allowPermissions([EUserPermissions.ADMIN], EUserPermissionsLevel.PROJECT, slug, project);

  // Bindings: per-project. Workspace schedulers: needed to populate the
  // install picker. Both keys include the workspace + project so a
  // navigation between projects refetches.
  const { data: bindings, mutate: mutateBindings } = useSWR<ISchedulerBinding[]>(
    slug && project ? ["scheduler-bindings", slug, project] : null,
    () => schedulerService.listBindings(slug, project)
  );
  const { data: schedulers } = useSWR<IScheduler[]>(slug ? ["schedulers", slug] : null, () =>
    schedulerService.listSchedulers(slug)
  );

  const [installOpen, setInstallOpen] = useState(false);
  const [editTarget, setEditTarget] = useState<ISchedulerBinding | null>(null);
  const [uninstallTarget, setUninstallTarget] = useState<ISchedulerBinding | null>(null);

  const pageTitle = currentProjectDetails?.name
    ? `${currentProjectDetails.name} · ${t("scheduler_bindings.title")}`
    : t("scheduler_bindings.title");

  if (workspaceUserInfo && !canManage) {
    return <NotAuthorizedView section="settings" isProjectView className="h-auto" />;
  }

  const rows = bindings ?? [];

  return (
    <SettingsContentWrapper header={<SchedulersProjectSettingsHeader />}>
      <PageHead title={pageTitle} />

      <div className="flex flex-col gap-6 p-6">
        <header className="flex items-start justify-between gap-4">
          <div>
            <h1 className="text-16 font-semibold text-primary">{t("scheduler_bindings.title")}</h1>
            <p className="mt-1 text-13 text-secondary">{t("scheduler_bindings.subtitle")}</p>
          </div>
          <Button onClick={() => setInstallOpen(true)}>{t("scheduler_bindings.install")}</Button>
        </header>

        <section className="rounded-md border border-subtle">
          <table className="w-full text-13">
            <thead className="bg-layer-1 text-left text-secondary">
              <tr>
                <th className="px-3 py-2">{t("scheduler_bindings.columns.name")}</th>
                <th className="px-3 py-2">{t("scheduler_bindings.columns.cron")}</th>
                <th className="px-3 py-2">{t("scheduler_bindings.columns.next_run")}</th>
                <th className="px-3 py-2">{t("scheduler_bindings.columns.last_run")}</th>
                <th className="px-3 py-2">{t("scheduler_bindings.columns.status")}</th>
                <th className="px-3 py-2">{t("scheduler_bindings.columns.updated")}</th>
                <th className="px-3 py-2"></th>
              </tr>
            </thead>
            <tbody>
              {rows.map((b) => (
                <BindingRow
                  key={b.id}
                  binding={b}
                  onEdit={() => setEditTarget(b)}
                  onUninstall={() => setUninstallTarget(b)}
                />
              ))}
              {rows.length === 0 && (
                <tr>
                  <td colSpan={7} className="px-3 py-8 text-center text-secondary">
                    {t("scheduler_bindings.list.empty")}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </section>
      </div>

      <InstallSchedulerBindingModal
        isOpen={installOpen}
        onClose={() => setInstallOpen(false)}
        workspaceSlug={slug}
        projectId={project}
        availableSchedulers={schedulers ?? []}
        existingBindings={rows}
        onInstalled={() => mutateBindings()}
      />
      <EditSchedulerBindingModal
        isOpen={!!editTarget}
        onClose={() => setEditTarget(null)}
        workspaceSlug={slug}
        projectId={project}
        binding={editTarget}
        onUpdated={() => mutateBindings()}
      />
      <UninstallSchedulerBindingModal
        isOpen={!!uninstallTarget}
        onClose={() => setUninstallTarget(null)}
        workspaceSlug={slug}
        projectId={project}
        binding={uninstallTarget}
        onUninstalled={() => mutateBindings()}
      />
    </SettingsContentWrapper>
  );
});

type RowProps = {
  binding: ISchedulerBinding;
  onEdit: () => void;
  onUninstall: () => void;
};

function BindingRow({ binding, onEdit, onUninstall }: RowProps) {
  const { t } = useTranslation();
  const formatTs = (ts: string | null) => (ts ? new Date(ts).toLocaleString() : t("scheduler_bindings.list.none_yet"));
  return (
    <tr className="border-t border-subtle">
      <td className="px-3 py-2 font-medium text-primary">
        {binding.scheduler_name}
        <div className="text-12 text-secondary">
          <code>{binding.scheduler_slug}</code>
        </div>
      </td>
      <td className="px-3 py-2 text-secondary">
        <code className="text-12">{binding.cron}</code>
      </td>
      <td className="px-3 py-2 text-secondary">{formatTs(binding.next_run_at)}</td>
      <td className="px-3 py-2 text-secondary">{formatTs(binding.last_run_ended_at)}</td>
      <td className="px-3 py-2">
        <Badge variant={binding.enabled ? "accent-primary" : "accent-neutral"} size="sm">
          {binding.enabled ? t("scheduler_bindings.status.enabled") : t("scheduler_bindings.status.disabled")}
        </Badge>
      </td>
      <td className="px-3 py-2 text-secondary">{new Date(binding.updated_at).toLocaleString()}</td>
      <td className="px-3 py-2 text-right">
        <div className="flex items-center justify-end gap-2">
          <Button variant="link-neutral" size="sm" onClick={onEdit}>
            {t("scheduler_bindings.actions.edit")}
          </Button>
          <Button variant="tertiary-danger" size="sm" onClick={onUninstall}>
            {t("scheduler_bindings.actions.uninstall")}
          </Button>
        </div>
      </td>
    </tr>
  );
}

export default SchedulerBindingsSettingsPage;
