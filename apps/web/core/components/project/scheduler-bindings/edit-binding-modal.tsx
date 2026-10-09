/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useRef, useState } from "react";
import { observer } from "mobx-react";
import type { SubmitHandler } from "react-hook-form";
import { useForm, useWatch } from "react-hook-form";
import { useSWRConfig } from "swr";
import { EUserPermissions, EUserPermissionsLevel } from "@pi-dash/constants";
import { useTranslation } from "@pi-dash/i18n";
import { Button } from "@pi-dash/propel/button";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import type { IScheduler, ISchedulerBinding, SchedulerOutcomeMode } from "@pi-dash/services";
import { SchedulerService } from "@pi-dash/services";
import { EModalPosition, EModalWidth, ModalCore } from "@pi-dash/ui";
import { diffSchedulerTemplate } from "@/components/schedulers/definition-helpers";
import { SchedulerDefinitionFields } from "@/components/schedulers/scheduler-definition-fields";
import { useUserPermissions } from "@/hooks/store/user";
import { BindingOutcomeModeField, DEFAULT_OUTCOME_MODE } from "./binding-outcome-mode-field";
import { BindingPodField } from "./binding-pod-field";
import { BindingScheduleFields } from "./binding-schedule-fields";
import { DEFAULT_SCHEDULER_COLOR, DEFAULT_TZID } from "./constants";
import { isoUTCToLocalInput, localToIsoUTC } from "./datetime-input";

interface EditFormValues {
  dtstart: string;
  tzid: string;
  rrule: string;
  extra_context: string;
  enabled: boolean;
  outcome_mode: SchedulerOutcomeMode;
  /** Pod id, or "" for the project default. */
  pod: string;
  // Scheduler template (workspace-level definition) — only edited while the
  // "Edit scheduler template" section is open.
  template_name: string;
  template_slug: string;
  template_description: string;
  template_prompt: string;
  template_color: string;
}

type Props = {
  isOpen: boolean;
  onClose: () => void;
  workspaceSlug: string;
  projectId: string;
  binding: ISchedulerBinding | null;
  onUpdated: (binding: ISchedulerBinding) => void;
};

type ApiError = Record<string, string | string[] | undefined> | null;

const TEMPLATE_FIELDS = ["name", "slug", "description", "prompt", "color"] as const;
const BINDING_ERROR_FIELDS = ["rrule", "dtstart", "tzid", "pod"] as const;

// Every SWR cache that renders a template's name, color, description or
// prompt. The template is shared, so they refresh workspace-wide.
const SCHEDULER_CACHE_KEYS = new Set([
  "schedulers",
  "scheduler-bindings",
  "scheduler-binding-detail",
  "scheduler-occurrences",
]);

const firstMessage = (value: string | string[] | undefined): string | undefined =>
  Array.isArray(value) ? value[0] : value;

const errorDetail = (err: ApiError, fields: readonly string[], fallback: string): string =>
  firstMessage(err?.error) ?? fields.map((f) => firstMessage(err?.[f])).find(Boolean) ?? fallback;

const schedulerService = new SchedulerService();

export const EditSchedulerBindingModal = observer(function EditSchedulerBindingModal(props: Props) {
  const { isOpen, onClose, workspaceSlug, projectId, binding, onUpdated } = props;
  const { t } = useTranslation();
  const { mutate: mutateCache } = useSWRConfig();
  const { allowPermissions } = useUserPermissions();
  // Updating a template is workspace-admin only; everyone else never sees the button.
  const canEditTemplate = allowPermissions([EUserPermissions.ADMIN], EUserPermissionsLevel.WORKSPACE, workspaceSlug);

  const {
    control,
    handleSubmit,
    reset,
    getValues,
    setValue,
    setError,
    clearErrors,
    formState: { errors, isSubmitting },
  } = useForm<EditFormValues>({
    defaultValues: {
      dtstart: "",
      tzid: DEFAULT_TZID,
      rrule: "",
      extra_context: "",
      enabled: true,
      outcome_mode: DEFAULT_OUTCOME_MODE,
      pod: "",
      template_name: "",
      template_slug: "",
      template_description: "",
      template_prompt: "",
      template_color: DEFAULT_SCHEDULER_COLOR,
    },
  });

  // The template as last loaded or saved; null while the section is closed.
  const [template, setTemplate] = useState<IScheduler | null>(null);
  const [templateLoading, setTemplateLoading] = useState(false);
  const [confirmDiscard, setConfirmDiscard] = useState(false);
  const templateOpen = !!template;
  const templateOpenRef = useRef(false);
  templateOpenRef.current = templateOpen;

  // Reset once per open, not on every `binding` identity change: a caller
  // may revalidate the binding while the dialog stays open after a partial
  // save, and that must not wipe the part still being edited.
  const loadedBindingId = useRef<string | null>(null);
  useEffect(() => {
    if (!isOpen) {
      loadedBindingId.current = null;
      return;
    }
    if (!binding || loadedBindingId.current === binding.id) return;
    loadedBindingId.current = binding.id;
    reset({
      dtstart: isoUTCToLocalInput(binding.dtstart),
      tzid: binding.tzid || DEFAULT_TZID,
      rrule: binding.rrule || "",
      extra_context: binding.extra_context ?? "",
      enabled: binding.enabled,
      outcome_mode: binding.outcome_mode ?? DEFAULT_OUTCOME_MODE,
      pod: binding.pod ?? "",
      template_name: "",
      template_slug: "",
      template_description: "",
      template_prompt: "",
      template_color: DEFAULT_SCHEDULER_COLOR,
    });
    setTemplate(null);
    setConfirmDiscard(false);
  }, [isOpen, binding, reset]);

  const watchedDtstart = useWatch({ control, name: "dtstart" }) ?? "";
  const watchedRrule = useWatch({ control, name: "rrule" }) ?? "";

  const fillTemplateFields = (scheduler: IScheduler) => {
    setValue("template_name", scheduler.name);
    setValue("template_slug", scheduler.slug);
    setValue("template_description", scheduler.description ?? "");
    setValue("template_prompt", scheduler.prompt);
    setValue("template_color", scheduler.color || DEFAULT_SCHEDULER_COLOR);
  };

  const templateChanges = (values: EditFormValues, saved: IScheduler) =>
    diffSchedulerTemplate(
      {
        name: values.template_name,
        description: values.template_description,
        prompt: values.template_prompt,
        color: values.template_color,
      },
      { ...saved, color: saved.color || DEFAULT_SCHEDULER_COLOR }
    );

  const closeTemplate = () => {
    setTemplate(null);
    setConfirmDiscard(false);
    clearErrors(TEMPLATE_FIELDS.map((f) => `template_${f}` as const));
  };

  const handleToggleTemplate = async () => {
    if (template) {
      // Collapsing discards unsaved template edits — confirm first if there are any.
      if (Object.keys(templateChanges(getValues(), template)).length > 0) setConfirmDiscard(true);
      else closeTemplate();
      return;
    }
    if (!binding || templateLoading) return;
    setTemplateLoading(true);
    try {
      const scheduler = await schedulerService.retrieveScheduler(workspaceSlug, binding.scheduler);
      fillTemplateFields(scheduler);
      setTemplate(scheduler);
    } catch (e: unknown) {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Something went wrong"),
        message: errorDetail(e as ApiError, [], t("Could not load the scheduler template.")),
      });
    } finally {
      setTemplateLoading(false);
    }
  };

  const refreshSchedulerCaches = () =>
    mutateCache((key) => Array.isArray(key) && SCHEDULER_CACHE_KEYS.has(key[0]) && key[1] === workspaceSlug);

  const handleFormSubmit: SubmitHandler<EditFormValues> = async (values) => {
    if (!binding) return;

    // Two records, two requests. Both are always attempted, so one failing
    // never hides whether the other was saved.
    const templatePayload = template ? templateChanges(values, template) : {};
    const hasTemplateChanges = template !== null && Object.keys(templatePayload).length > 0;
    let templateError: ApiError | undefined;
    if (template && hasTemplateChanges) {
      try {
        const saved = await schedulerService.updateScheduler(workspaceSlug, template.id, templatePayload);
        setTemplate(saved);
        fillTemplateFields(saved);
      } catch (e: unknown) {
        templateError = (e ?? {}) as ApiError;
      }
    }
    const templateSaved = hasTemplateChanges && templateError === undefined;

    let updated: ISchedulerBinding | undefined;
    let bindingError: ApiError | undefined;
    try {
      updated = await schedulerService.updateBinding(workspaceSlug, projectId, binding.id, {
        dtstart: localToIsoUTC(values.dtstart),
        tzid: values.tzid.trim() || DEFAULT_TZID,
        rrule: values.rrule.trim(),
        extra_context: values.extra_context.trim(),
        enabled: values.enabled,
        outcome_mode: values.outcome_mode,
        pod: values.pod || null,
      });
    } catch (e: unknown) {
      bindingError = (e ?? {}) as ApiError;
    }

    if (templateSaved || (updated && templateError !== undefined)) void refreshSchedulerCaches();

    if (updated && templateError === undefined) {
      setToast({
        type: TOAST_TYPE.SUCCESS,
        title: templateSaved ? t("Install and scheduler template updated") : t("Install updated"),
        message: t("Subsequent runs use the new settings."),
      });
      onUpdated(updated);
      onClose();
      return;
    }

    // Something failed: the dialog stays open with the failed part still
    // editable, and server field errors land under their fields.
    if (templateError !== undefined) {
      for (const field of TEMPLATE_FIELDS) {
        const message = firstMessage(templateError?.[field]);
        if (message) setError(`template_${field}`, { type: "server", message });
      }
    }
    if (bindingError !== undefined) {
      for (const field of ["rrule", "dtstart"] as const) {
        const message = firstMessage(bindingError?.[field]);
        if (message) setError(field, { type: "server", message });
      }
    }
    const templateDetail = errorDetail(templateError ?? null, TEMPLATE_FIELDS, t("Could not update the scheduler."));
    const bindingDetail = errorDetail(bindingError ?? null, BINDING_ERROR_FIELDS, t("Could not update the install."));

    if (updated) {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Scheduler template not saved"),
        message: t("The install settings were saved. The scheduler template was not saved: {detail}", {
          detail: templateDetail,
        }),
      });
    } else if (templateSaved) {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Install settings not saved"),
        message: t("The scheduler template was saved. The install settings were not saved: {detail}", {
          detail: bindingDetail,
        }),
      });
    } else if (templateError !== undefined) {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Nothing was saved"),
        message: t("Install settings: {installDetail} Scheduler template: {templateDetail}", {
          installDetail: bindingDetail,
          templateDetail,
        }),
      });
    } else {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Something went wrong"),
        message: bindingDetail,
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

        <BindingOutcomeModeField control={control} name="outcome_mode" />

        <BindingPodField control={control} name="pod" projectId={projectId} />

        {canEditTemplate && (
          <div className="flex flex-col gap-4 border-t border-subtle pt-4">
            <div>
              <Button
                variant="secondary"
                type="button"
                onClick={handleToggleTemplate}
                loading={templateLoading}
                disabled={templateLoading || isSubmitting}
                aria-expanded={templateOpen}
              >
                {t("Edit scheduler template")}
              </Button>
            </div>

            {confirmDiscard && (
              <div
                role="alert"
                className="flex flex-wrap items-center justify-between gap-3 rounded-md border border-subtle bg-layer-1 p-3"
              >
                <span className="text-13 text-primary">{t("Discard your unsaved scheduler template changes?")}</span>
                <div className="flex gap-2">
                  <Button variant="secondary" size="sm" type="button" onClick={() => setConfirmDiscard(false)}>
                    {t("Keep editing")}
                  </Button>
                  <Button variant="error-fill" size="sm" type="button" onClick={closeTemplate}>
                    {t("Discard changes")}
                  </Button>
                </div>
              </div>
            )}

            {template && (
              <>
                <p role="note" className="rounded-md border border-subtle bg-layer-1 p-3 text-13 text-secondary">
                  {t(
                    "This template is shared. Changes apply to every project this scheduler is installed on ({count, plural, one {# active install} other {# active installs}}).",
                    { count: template.active_binding_count }
                  )}
                </p>
                <SchedulerDefinitionFields
                  control={control}
                  errors={errors}
                  nameName="template_name"
                  slugName="template_slug"
                  descriptionName="template_description"
                  promptName="template_prompt"
                  colorName="template_color"
                  slugDisabled
                  isActive={() => templateOpenRef.current}
                />
              </>
            )}
          </div>
        )}

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
