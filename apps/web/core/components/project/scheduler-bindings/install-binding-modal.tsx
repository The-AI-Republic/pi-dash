/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useMemo, useRef } from "react";
import { observer } from "mobx-react";
import type { SubmitHandler } from "react-hook-form";
import { Controller, useForm, useWatch } from "react-hook-form";
import { useTranslation } from "@pi-dash/i18n";
import { Button } from "@pi-dash/propel/button";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import type { IScheduler, ISchedulerBinding } from "@pi-dash/services";
import { SchedulerService } from "@pi-dash/services";
import { EModalPosition, EModalWidth, ModalCore } from "@pi-dash/ui";
import { BindingPodField } from "./binding-pod-field";
import { BindingScheduleFields } from "./binding-schedule-fields";
import { DEFAULT_TZID } from "./constants";
import { defaultDtstartLocal, localToIsoUTC } from "./datetime-input";

interface InstallFormValues {
  scheduler: string;
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
  availableSchedulers: IScheduler[];
  existingBindings: ISchedulerBinding[];
  onInstalled: (binding: ISchedulerBinding) => void;
};

const DEFAULT_VALUES = (): InstallFormValues => ({
  scheduler: "",
  dtstart: defaultDtstartLocal(),
  tzid: Intl.DateTimeFormat().resolvedOptions().timeZone || DEFAULT_TZID,
  rrule: "FREQ=DAILY",
  extra_context: "",
  enabled: true,
  pod: "",
});

const schedulerService = new SchedulerService();

export const InstallSchedulerBindingModal = observer(function InstallSchedulerBindingModal(props: Props) {
  const { isOpen, onClose, workspaceSlug, projectId, availableSchedulers, existingBindings, onInstalled } = props;
  const { t } = useTranslation();

  const {
    control,
    handleSubmit,
    reset,
    formState: { errors, isSubmitting },
  } = useForm<InstallFormValues>({ defaultValues: DEFAULT_VALUES() });

  // SWR may revalidate workspaceSchedulers while the modal is open; only
  // seed on the closed→open edge so the user's in-progress edit isn't wiped.
  const installable = useMemo(() => {
    const boundIds = new Set(existingBindings.map((b) => b.scheduler));
    return availableSchedulers.filter((s) => s.is_enabled && !boundIds.has(s.id));
  }, [availableSchedulers, existingBindings]);

  const wasOpen = useRef(false);
  useEffect(() => {
    if (isOpen && !wasOpen.current) {
      reset({
        ...DEFAULT_VALUES(),
        scheduler: installable[0]?.id ?? "",
      });
    }
    wasOpen.current = isOpen;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isOpen, reset]);

  const watchedDtstart = useWatch({ control, name: "dtstart" }) ?? "";
  const watchedRrule = useWatch({ control, name: "rrule" }) ?? "";

  const handleFormSubmit: SubmitHandler<InstallFormValues> = async (values) => {
    try {
      const binding = await schedulerService.createBinding(workspaceSlug, projectId, {
        scheduler: values.scheduler,
        project: projectId,
        dtstart: localToIsoUTC(values.dtstart),
        tzid: values.tzid.trim() || DEFAULT_TZID,
        rrule: values.rrule.trim(),
        extra_context: values.extra_context.trim(),
        enabled: values.enabled,
        pod: values.pod || null,
      });
      setToast({
        type: TOAST_TYPE.SUCCESS,
        title: t("Scheduler installed"),
        message: t("It will fire on the configured schedule."),
      });
      onInstalled(binding);
      onClose();
    } catch (e: unknown) {
      const err = e as {
        error?: string;
        rrule?: string[];
        dtstart?: string[];
        tzid?: string[];
        scheduler?: string[];
        pod?: string[];
      } | null;
      const detail =
        err?.error ??
        err?.rrule?.[0] ??
        err?.dtstart?.[0] ??
        err?.tzid?.[0] ??
        err?.scheduler?.[0] ??
        err?.pod?.[0] ??
        t("Could not install the scheduler.");
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Something went wrong"),
        message: detail,
      });
    }
  };

  if (installable.length === 0) {
    return (
      <ModalCore isOpen={isOpen} handleClose={onClose} position={EModalPosition.CENTER} width={EModalWidth.XL}>
        <div className="flex flex-col gap-4 p-5">
          <div className="text-18 font-medium text-primary">{t("No schedulers available")}</div>
          <p className="text-13 text-secondary">
            {t(
              "Either every workspace scheduler is already installed on this project, or your workspace admin hasn't enabled any. Visit Workspace → Schedulers to manage the catalog."
            )}
          </p>
          <div className="flex justify-end">
            <Button variant="secondary" onClick={onClose}>
              {t("Cancel")}
            </Button>
          </div>
        </div>
      </ModalCore>
    );
  }

  return (
    <ModalCore isOpen={isOpen} handleClose={onClose} position={EModalPosition.CENTER} width={EModalWidth.XXL}>
      <form onSubmit={handleSubmit(handleFormSubmit)} className="flex flex-col gap-5 p-5">
        <div className="text-18 font-medium text-primary">{t("Install scheduler")}</div>

        <div className="flex flex-col gap-1">
          <label htmlFor="binding-scheduler" className="text-13 font-medium text-primary">
            {t("Scheduler")}
          </label>
          <Controller
            control={control}
            name="scheduler"
            rules={{ required: t("Pick a scheduler.") }}
            render={({ field }) => (
              <select
                {...field}
                id="binding-scheduler"
                className="rounded-md border border-subtle bg-surface-1 px-3 py-2 text-13 text-primary focus:ring-1 focus:ring-accent-strong focus:outline-none"
              >
                {installable.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.name} ({s.slug})
                  </option>
                ))}
              </select>
            )}
          />
          <p className="text-12 text-secondary">
            {t("Pick from your workspace's enabled schedulers. Already-installed ones aren't listed.")}
          </p>
          {errors.scheduler && <span className="text-12 text-danger-primary">{errors.scheduler.message}</span>}
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
            {isSubmitting ? t("Installing…") : t("Install")}
          </Button>
        </div>
      </form>
    </ModalCore>
  );
});
