/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { observer } from "mobx-react";
import useSWR from "swr";
// pi dash imports
import { EUserPermissions, EUserPermissionsLevel } from "@pi-dash/constants";
import { useTranslation } from "@pi-dash/i18n";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import type { IScheduler, ISchedulerBinding } from "@pi-dash/services";
import { SchedulerService } from "@pi-dash/services";
import { Button, ToggleSwitch } from "@pi-dash/ui";
// components
import { EditSchedulerBindingModal } from "@/components/project/scheduler-bindings/edit-binding-modal";
import { InstallSchedulerBindingModal } from "@/components/project/scheduler-bindings/install-binding-modal";
import { UninstallSchedulerBindingModal } from "@/components/project/scheduler-bindings/uninstall-binding-modal";
// hooks
import { useUserPermissions } from "@/hooks/store/user";

const schedulerService = new SchedulerService();

type Props = {
  workspaceSlug: string;
  projectId: string;
};

/**
 * Shared scheduler-bindings UI rendered from both the project settings page
 * and the project sidebar entry. Mutations (install/edit/uninstall/toggle)
 * are gated on PROJECT ADMIN; non-admins see a read-only list.
 */
export const SchedulerBindingsPanel = observer(function SchedulerBindingsPanel(props: Props) {
  const { workspaceSlug, projectId } = props;
  const { allowPermissions } = useUserPermissions();
  const { t } = useTranslation();

  const canManage = allowPermissions([EUserPermissions.ADMIN], EUserPermissionsLevel.PROJECT, workspaceSlug, projectId);

  const { data: bindings, mutate: mutateBindings } = useSWR<ISchedulerBinding[]>(
    workspaceSlug && projectId ? ["scheduler-bindings", workspaceSlug, projectId] : null,
    () => schedulerService.listBindings(workspaceSlug, projectId)
  );
  const { data: schedulers } = useSWR<IScheduler[]>(workspaceSlug ? ["schedulers", workspaceSlug] : null, () =>
    schedulerService.listSchedulers(workspaceSlug)
  );

  const [installOpen, setInstallOpen] = useState(false);
  const [editTarget, setEditTarget] = useState<ISchedulerBinding | null>(null);
  const [uninstallTarget, setUninstallTarget] = useState<ISchedulerBinding | null>(null);

  const rows = bindings ?? [];

  return (
    <>
      <div className="flex flex-col gap-6 p-6">
        <header className="flex items-start justify-between gap-4">
          <div>
            <h1 className="text-16 font-semibold text-primary">{t("scheduler_bindings.title")}</h1>
            <p className="mt-1 text-13 text-secondary">{t("scheduler_bindings.subtitle")}</p>
          </div>
          {canManage && <Button onClick={() => setInstallOpen(true)}>{t("scheduler_bindings.install")}</Button>}
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
                  canManage={canManage}
                  onEdit={() => setEditTarget(b)}
                  onUninstall={() => setUninstallTarget(b)}
                  onToggle={async (next) => {
                    try {
                      await mutateBindings(
                        async (current) => {
                          const updated = await schedulerService.updateBinding(workspaceSlug, projectId, b.id, {
                            enabled: next,
                          });
                          return (current ?? []).map((row) => (row.id === b.id ? updated : row));
                        },
                        {
                          // SWR diffs optimisticData by reference, so we need a
                          // fresh array AND a fresh object for the changed row.
                          // Build the new array imperatively to dodge oxlint's
                          // spread-in-map rule.
                          optimisticData: (current) => {
                            const arr = current ?? [];
                            const idx = arr.findIndex((row) => row.id === b.id);
                            if (idx === -1) return arr;
                            const out = arr.slice();
                            out[idx] = Object.assign({}, arr[idx], { enabled: next });
                            return out;
                          },
                          rollbackOnError: true,
                          revalidate: false,
                        }
                      );
                      setToast({
                        type: TOAST_TYPE.SUCCESS,
                        title: t("scheduler_bindings.toast.updated_title"),
                        message: next
                          ? t("scheduler_bindings.toast.enabled_message")
                          : t("scheduler_bindings.toast.disabled_message"),
                      });
                    } catch (e: unknown) {
                      const err = e as { error?: string } | null;
                      setToast({
                        type: TOAST_TYPE.ERROR,
                        title: t("scheduler_bindings.toast.error_title"),
                        message: err?.error ?? t("scheduler_bindings.toast.update_failed"),
                      });
                    }
                  }}
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
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        availableSchedulers={schedulers ?? []}
        existingBindings={rows}
        onInstalled={() => mutateBindings()}
      />
      <EditSchedulerBindingModal
        isOpen={!!editTarget}
        onClose={() => setEditTarget(null)}
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        binding={editTarget}
        onUpdated={() => mutateBindings()}
      />
      <UninstallSchedulerBindingModal
        isOpen={!!uninstallTarget}
        onClose={() => setUninstallTarget(null)}
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        binding={uninstallTarget}
        onUninstalled={() => mutateBindings()}
      />
    </>
  );
});

type RowProps = {
  binding: ISchedulerBinding;
  canManage: boolean;
  onEdit: () => void;
  onUninstall: () => void;
  onToggle: (next: boolean) => Promise<void>;
};

function BindingRow({ binding, canManage, onEdit, onUninstall, onToggle }: RowProps) {
  const { t } = useTranslation();
  const [toggling, setToggling] = useState(false);

  const formatTs = (ts: string | null) => (ts ? new Date(ts).toLocaleString() : t("scheduler_bindings.list.none_yet"));

  // Wraps the parent's onToggle so the switch can render a disabled state
  // during the in-flight PATCH (prevents a double-click from racing two
  // requests).
  const handleToggle = async (next: boolean) => {
    if (toggling) return;
    setToggling(true);
    try {
      await onToggle(next);
    } finally {
      setToggling(false);
    }
  };

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
        <div className="flex items-center gap-2">
          <ToggleSwitch
            value={binding.enabled}
            onChange={handleToggle}
            disabled={toggling || !canManage}
            aria-label={
              binding.enabled ? t("scheduler_bindings.actions.disable") : t("scheduler_bindings.actions.enable")
            }
          />
          <span className="text-12 text-secondary">
            {binding.enabled ? t("scheduler_bindings.status.enabled") : t("scheduler_bindings.status.disabled")}
          </span>
        </div>
      </td>
      <td className="px-3 py-2 text-secondary">{new Date(binding.updated_at).toLocaleString()}</td>
      <td className="px-3 py-2 text-right">
        {canManage && (
          <div className="flex items-center justify-end gap-2">
            <Button variant="link-neutral" size="sm" onClick={onEdit}>
              {t("scheduler_bindings.actions.edit")}
            </Button>
            <Button variant="tertiary-danger" size="sm" onClick={onUninstall}>
              {t("scheduler_bindings.actions.uninstall")}
            </Button>
          </div>
        )}
      </td>
    </tr>
  );
}
