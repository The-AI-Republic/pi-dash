/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { observer } from "mobx-react";
// pi dash imports
import { useTranslation } from "@pi-dash/i18n";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import type { ISchedulerBinding } from "@pi-dash/services";
import { SchedulerService } from "@pi-dash/services";
import { AlertModalCore } from "@pi-dash/ui";

type Props = {
  isOpen: boolean;
  onClose: () => void;
  workspaceSlug: string;
  projectId: string;
  binding: ISchedulerBinding | null;
  onUninstalled: (bindingId: string) => void;
};

const schedulerService = new SchedulerService();

export const UninstallSchedulerBindingModal = observer(function UninstallSchedulerBindingModal(props: Props) {
  const { isOpen, onClose, workspaceSlug, projectId, binding, onUninstalled } = props;
  const { t } = useTranslation();
  const [submitting, setSubmitting] = useState(false);

  const handleUninstall = async () => {
    if (!binding) return;
    setSubmitting(true);
    try {
      await schedulerService.destroyBinding(workspaceSlug, projectId, binding.id);
      setToast({
        type: TOAST_TYPE.SUCCESS,
        title: t("scheduler_bindings.toast.uninstalled_title"),
        message: t("scheduler_bindings.toast.uninstalled_message"),
      });
      onUninstalled(binding.id);
      onClose();
    } catch (e: unknown) {
      const err = e as { error?: string } | null;
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("scheduler_bindings.toast.error_title"),
        message: err?.error ?? t("scheduler_bindings.toast.uninstall_failed"),
      });
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <AlertModalCore
      isOpen={isOpen}
      handleClose={() => (submitting ? null : onClose())}
      handleSubmit={handleUninstall}
      isSubmitting={submitting}
      title={t("scheduler_bindings.uninstall_modal.title")}
      content={
        <>
          <p>{t("scheduler_bindings.uninstall_modal.body")}</p>
          {binding && <p className="mt-2 font-medium text-primary">{binding.scheduler_name}</p>}
        </>
      }
      primaryButtonText={{
        default: t("scheduler_bindings.uninstall_modal.confirm"),
        loading: t("scheduler_bindings.uninstall_modal.confirm"),
      }}
    />
  );
});
