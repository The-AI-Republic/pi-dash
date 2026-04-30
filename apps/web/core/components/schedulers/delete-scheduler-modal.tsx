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
import type { IScheduler } from "@pi-dash/services";
import { AlertModalCore } from "@pi-dash/ui";
// hooks
import { useScheduler } from "@/hooks/store/use-scheduler";

type Props = {
  isOpen: boolean;
  onClose: () => void;
  workspaceSlug: string;
  scheduler: IScheduler | null;
  onDeleted?: () => void;
};

export const DeleteSchedulerModal = observer(function DeleteSchedulerModal(props: Props) {
  const { isOpen, onClose, workspaceSlug, scheduler, onDeleted } = props;
  const { deleteScheduler } = useScheduler();
  const { t } = useTranslation();
  const [submitting, setSubmitting] = useState(false);

  const handleDelete = async () => {
    if (!scheduler) return;
    setSubmitting(true);
    try {
      await deleteScheduler(workspaceSlug, scheduler.id);
      setToast({
        type: TOAST_TYPE.SUCCESS,
        title: t("schedulers.toast.deleted_title"),
        message: t("schedulers.toast.deleted_message"),
      });
      onDeleted?.();
      onClose();
    } catch (e: unknown) {
      const err = e as { error?: string } | null;
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("schedulers.toast.error_title"),
        message: err?.error ?? t("schedulers.toast.delete_failed"),
      });
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <AlertModalCore
      isOpen={isOpen}
      handleClose={() => (submitting ? null : onClose())}
      handleSubmit={handleDelete}
      isSubmitting={submitting}
      title={t("schedulers.delete.confirm_title")}
      content={
        <>
          <p>{t("schedulers.delete.confirm_body")}</p>
          {scheduler && <p className="mt-2 font-medium text-primary">{scheduler.name}</p>}
        </>
      }
      primaryButtonText={{
        default: t("schedulers.delete.confirm"),
        loading: t("schedulers.delete.confirm"),
      }}
    />
  );
});
