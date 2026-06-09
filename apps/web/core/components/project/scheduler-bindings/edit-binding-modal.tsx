/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect } from "react";
import { observer } from "mobx-react";
import type { SubmitHandler } from "react-hook-form";
import { useForm, useWatch } from "react-hook-form";
import { useTranslation } from "@pi-dash/i18n";
import { Button } from "@pi-dash/propel/button";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import type { ISchedulerBinding } from "@pi-dash/services";
import { SchedulerService } from "@pi-dash/services";
import { EModalPosition, EModalWidth, ModalCore } from "@pi-dash/ui";
import { BindingPodField } from "./binding-pod-field";
import { BindingScheduleFields } from "./binding-schedule-fields";
import { DEFAULT_TZID } from "./constants";
import { isoUTCToLocalInput, localToIsoUTC } from "./datetime-input";

interface EditFormValues {
  dtstart: string;
  tzid: string;
  rrule: string;
  extra_context: string;
  enabled: boolean;
  /** Pod id, or "" for the project default. */
  pod: string;
}

type Props = {
  isOpen: boolean;
  onClose: () => void;
  workspaceSlug: string;
  projectId: string;
  binding: ISchedulerBinding | null;
  onUpdated: (binding: ISchedulerBinding) => void;
};

const schedulerService = new SchedulerService();

export const EditSchedulerBindingModal = observer(function EditSchedulerBindingModal(props: Props) {
  const { isOpen, onClose, workspaceSlug, projectId, binding, onUpdated } = props;
  const { t } = useTranslation();

  const {
    control,
    handleSubmit,
    reset,
    formState: { errors, isSubmitting },
  } = useForm<EditFormValues>({
    defaultValues: { dtstart: "", tzid: DEFAULT_TZID, rrule: "", extra_context: "", enabled: true, pod: "" },
  });

  useEffect(() => {
    if (!isOpen || !binding) return;
    reset({
      dtstart: isoUTCToLocalInput(binding.dtstart),
      tzid: binding.tzid || DEFAULT_TZID,
      rrule: binding.rrule || "",
      extra_context: binding.extra_context ?? "",
      enabled: binding.enabled,
      pod: binding.pod ?? "",
    });
  }, [isOpen, binding, reset]);

  const watchedDtstart = useWatch({ control, name: "dtstart" }) ?? "";
  const watchedRrule = useWatch({ control, name: "rrule" }) ?? "";

  const handleFormSubmit: SubmitHandler<EditFormValues> = async (values) => {
    if (!binding) return;
    try {
      const updated = await schedulerService.updateBinding(workspaceSlug, projectId, binding.id, {
        dtstart: localToIsoUTC(values.dtstart),
        tzid: values.tzid.trim() || DEFAULT_TZID,
        rrule: values.rrule.trim(),
        extra_context: values.extra_context.trim(),
        enabled: values.enabled,
        pod: values.pod || null,
      });
      setToast({
        type: TOAST_TYPE.SUCCESS,
        title: t("Install updated"),
        message: t("Subsequent runs use the new settings."),
      });
      onUpdated(updated);
      onClose();
    } catch (e: unknown) {
      const err = e as {
        error?: string;
        rrule?: string[];
        dtstart?: string[];
        tzid?: string[];
        pod?: string[];
      } | null;
      const detail =
        err?.error ??
        err?.rrule?.[0] ??
        err?.dtstart?.[0] ??
        err?.tzid?.[0] ??
        err?.pod?.[0] ??
        t("Could not update the install.");
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Something went wrong"),
        message: detail,
      });
    }
  };

  return (
    <ModalCore isOpen={isOpen} handleClose={onClose} position={EModalPosition.CENTER} width={EModalWidth.XXL}>
      <form onSubmit={handleSubmit(handleFormSubmit)} className="flex flex-col gap-5 p-5">
        <div className="text-18 font-medium text-primary">
          {t("Edit scheduler install")}
          {binding && <span className="ml-2 text-13 text-secondary">— {binding.scheduler_name}</span>}
        </div>

        <BindingScheduleFields
          control={control}
          errors={errors}
          dtstartName="dtstart"
          tzidName="tzid"
          rruleName="rrule"
          extraContextName="extra_context"
          enabledName="enabled"
          watchDtstart={watchedDtstart}
          watchRrule={watchedRrule}
        />

        <BindingPodField control={control} name="pod" projectId={projectId} />

        <div className="flex justify-end gap-2">
          <Button variant="secondary" onClick={onClose} disabled={isSubmitting}>
            {t("Cancel")}
          </Button>
          <Button type="submit" loading={isSubmitting} disabled={isSubmitting}>
            {isSubmitting ? t("Saving…") : t("Save")}
          </Button>
        </div>
      </form>
    </ModalCore>
  );
});
