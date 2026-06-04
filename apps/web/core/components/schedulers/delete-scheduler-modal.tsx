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
        title: t("Scheduler deleted"),
        message: t("Active bindings have stopped firing."),
      });
      onDeleted?.();
      onClose();
    } catch (e: unknown) {
      const err = e as { error?: string } | null;
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Something went wrong"),
        message: err?.error ?? t("Could not delete the scheduler."),
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
      title={t("Delete scheduler?")}
      content={
        <>
          <p>{t("This soft-deletes the scheduler. Any active project bindings will stop firing. The slug becomes available for re-creation.")}</p>
          {scheduler && <p className="mt-2 font-medium text-primary">{scheduler.name}</p>}
        </>
      }
      primaryButtonText={{
        default: t("Delete"),
        loading: t("Delete"),
      }}
    />
  );
});
