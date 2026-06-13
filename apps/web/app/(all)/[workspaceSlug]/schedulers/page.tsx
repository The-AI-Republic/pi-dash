/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { observer } from "mobx-react";
import { useParams } from "react-router";
import useSWR from "swr";
import { EUserPermissions, EUserPermissionsLevel } from "@pi-dash/constants";
import { useTranslation } from "@pi-dash/i18n";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import type { IScheduler } from "@pi-dash/services";
import { Badge, Button } from "@pi-dash/ui";
import { PageHead } from "@/components/core/page-title";
import { DeleteSchedulerModal } from "@/components/schedulers/delete-scheduler-modal";
import { InstallSchedulerOnProjectsModal } from "@/components/schedulers/install-scheduler-on-projects-modal";
import { SchedulerFormModal } from "@/components/schedulers/scheduler-form-modal";
import { useScheduler } from "@/hooks/store/use-scheduler";
import { useUserPermissions } from "@/hooks/store/user";
import { useWorkspace } from "@/hooks/store/use-workspace";

const SchedulersListPage = observer(function SchedulersListPage() {
  const { workspaceSlug } = useParams<{ workspaceSlug: string }>();
  const { currentWorkspace } = useWorkspace();
  const { allowPermissions } = useUserPermissions();
  const { t } = useTranslation();

  const schedulerStore = useScheduler();

  const slug = workspaceSlug ?? "";
  const { data: schedulers, mutate } = useSWR<IScheduler[]>(slug ? ["schedulers", slug] : null, () =>
    schedulerStore.fetchSchedulers(slug)
  );

  const canEdit = allowPermissions([EUserPermissions.ADMIN], EUserPermissionsLevel.WORKSPACE, slug);

  const [createOpen, setCreateOpen] = useState(false);
  const [editTarget, setEditTarget] = useState<IScheduler | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<IScheduler | null>(null);
  const [installTarget, setInstallTarget] = useState<IScheduler | null>(null);

  const rows = schedulers ?? [];

  const handleCreate = async (values: {
    slug: string;
    name: string;
    description: string;
    prompt: string;
    color: string;
    is_enabled: boolean;
  }) => {
    try {
      await schedulerStore.createScheduler(slug, values);
      setCreateOpen(false);
      mutate();
      setToast({
        type: TOAST_TYPE.SUCCESS,
        title: t("Scheduler created"),
        message: t("Project admins can now install it on their projects."),
      });
    } catch (e: unknown) {
      const err = e as { error?: string; slug?: string[]; name?: string[]; prompt?: string[] } | null;
      const detail =
        err?.error ?? err?.slug?.[0] ?? err?.name?.[0] ?? err?.prompt?.[0] ?? t("Could not create the scheduler.");
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Something went wrong"),
        message: detail,
      });
    }
  };

  const handleEditSubmit = async (values: {
    slug: string;
    name: string;
    description: string;
    prompt: string;
    color: string;
    is_enabled: boolean;
  }) => {
    if (!editTarget) return;
    try {
      // Slug is read-only on the backend; only send the editable fields.
      await schedulerStore.updateScheduler(slug, editTarget.id, {
        name: values.name,
        description: values.description,
        prompt: values.prompt,
        color: values.color,
        is_enabled: values.is_enabled,
      });
      setEditTarget(null);
      mutate();
      setToast({
        type: TOAST_TYPE.SUCCESS,
        title: t("Scheduler updated"),
        message: t("Subsequent runs will use the updated definition."),
      });
    } catch (e: unknown) {
      const err = e as { error?: string; name?: string[]; prompt?: string[] } | null;
      const detail = err?.error ?? err?.name?.[0] ?? err?.prompt?.[0] ?? t("Could not update the scheduler.");
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Something went wrong"),
        message: detail,
      });
    }
  };

  const pageTitle = currentWorkspace?.name ? `${currentWorkspace.name} · ${t("Schedulers")}` : t("Schedulers");

  return (
    <div className="flex flex-col gap-6 p-6">
      <PageHead title={pageTitle} />

      <header className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-16 font-semibold text-primary">{t("Schedulers")}</h1>
          <p className="mt-1 text-13 text-secondary">
            {t(
              "Reusable scheduler definitions for this workspace. Install one on a project to run its prompt against the project on a cron."
            )}
          </p>
        </div>
        {canEdit && <Button onClick={() => setCreateOpen(true)}>{t("New scheduler")}</Button>}
      </header>

      <section className="rounded-md border border-subtle">
        <table className="w-full text-13">
          <thead className="bg-layer-1 text-left text-secondary">
            <tr>
              <th className="px-3 py-2">{t("Name")}</th>
              <th className="px-3 py-2">{t("Slug")}</th>
              <th className="px-3 py-2">{t("Source")}</th>
              <th className="px-3 py-2">{t("Installs")}</th>
              <th className="px-3 py-2">{t("Status")}</th>
              <th className="px-3 py-2">{t("Updated")}</th>
              <th className="px-3 py-2"></th>
            </tr>
          </thead>
          <tbody>
            {rows.map((s) => (
              <SchedulerRow
                key={s.id}
                scheduler={s}
                canEdit={canEdit}
                onInstall={() => setInstallTarget(s)}
                onEdit={() => setEditTarget(s)}
                onDelete={() => setDeleteTarget(s)}
              />
            ))}
            {rows.length === 0 && (
              <tr>
                <td colSpan={7} className="px-3 py-8 text-center text-secondary">
                  {t("No schedulers in this workspace yet. Click “New scheduler” to create one.")}
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </section>

      <SchedulerFormModal isOpen={createOpen} onClose={() => setCreateOpen(false)} onSubmit={handleCreate} />
      <SchedulerFormModal
        isOpen={!!editTarget}
        onClose={() => setEditTarget(null)}
        onSubmit={handleEditSubmit}
        scheduler={editTarget}
      />
      <DeleteSchedulerModal
        isOpen={!!deleteTarget}
        onClose={() => setDeleteTarget(null)}
        workspaceSlug={slug}
        scheduler={deleteTarget}
        onDeleted={() => mutate()}
      />
      <InstallSchedulerOnProjectsModal
        isOpen={!!installTarget}
        onClose={() => setInstallTarget(null)}
        workspaceSlug={slug}
        scheduler={installTarget}
        onInstalled={() => mutate()}
      />
    </div>
  );
});

type RowProps = {
  scheduler: IScheduler;
  canEdit: boolean;
  onInstall: () => void;
  onEdit: () => void;
  onDelete: () => void;
};

function SchedulerRow({ scheduler, canEdit, onInstall, onEdit, onDelete }: RowProps) {
  const { t } = useTranslation();
  return (
    <tr className="border-t border-subtle">
      <td className="px-3 py-2 font-medium text-primary">{scheduler.name}</td>
      <td className="px-3 py-2 text-secondary">
        <code className="text-12">{scheduler.slug}</code>
      </td>
      <td className="px-3 py-2">
        <Badge variant="accent-neutral" size="sm">
          {scheduler.source === "manifest" ? t("Manifest") : t("Built-in")}
        </Badge>
      </td>
      <td className="px-3 py-2 text-secondary">
        {t("{count, plural, one {# install} other {# installs}}", { count: scheduler.active_binding_count })}
      </td>
      <td className="px-3 py-2">
        <Badge variant={scheduler.is_enabled ? "accent-primary" : "accent-neutral"} size="sm">
          {scheduler.is_enabled ? t("Enabled") : t("Disabled")}
        </Badge>
      </td>
      <td className="px-3 py-2 text-secondary">{new Date(scheduler.updated_at).toLocaleString()}</td>
      <td className="px-3 py-2 text-right">
        {canEdit && (
          <div className="flex items-center justify-end gap-2">
            {scheduler.is_enabled && (
              <Button variant="link-primary" size="sm" onClick={onInstall}>
                {t("Install")}
              </Button>
            )}
            <Button variant="link-neutral" size="sm" onClick={onEdit}>
              {t("Edit")}
            </Button>
            <Button variant="tertiary-danger" size="sm" onClick={onDelete}>
              {t("Delete")}
            </Button>
          </div>
        )}
      </td>
    </tr>
  );
}

export default SchedulersListPage;
