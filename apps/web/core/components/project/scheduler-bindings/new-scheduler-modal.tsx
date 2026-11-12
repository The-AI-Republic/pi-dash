/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useMemo, useRef, useState } from "react";
import { observer } from "mobx-react";
import type { SubmitHandler } from "react-hook-form";
import { Controller, useForm, useWatch } from "react-hook-form";
import { useTranslation } from "@pi-dash/i18n";
import { Button } from "@pi-dash/propel/button";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import type { IScheduler, ISchedulerBinding, SchedulerOutcomeMode } from "@pi-dash/services";
import { SchedulerService } from "@pi-dash/services";
import { EModalPosition, EModalWidth, ModalCore } from "@pi-dash/ui";
import { deriveSchedulerSlug } from "@/components/schedulers/definition-helpers";
import { SchedulerDefinitionFields } from "@/components/schedulers/scheduler-definition-fields";
import { BindingOutcomeModeField, DEFAULT_OUTCOME_MODE } from "./binding-outcome-mode-field";
import { BindingPodField } from "./binding-pod-field";
import { BindingScheduleFields } from "./binding-schedule-fields";
import { DEFAULT_TZID, SCHEDULER_COLOR_PALETTE as COLOR_PALETTE } from "./constants";
import { defaultDtstartLocal, localToIsoUTC } from "./datetime-input";

/** Which path the modal is on: install a catalog scheduler, or author one. */
export type NewSchedulerMode = "install" | "create";

interface NewSchedulerFormValues {
  // install mode
  scheduler: string;
  // create mode — definition fields
  name: string;
  slug: string;
  description: string;
  prompt: string;
  color: string;
  // binding fields, shared by both modes
  dtstart: string;
  tzid: string;
  rrule: string;
  extra_context: string;
  enabled: boolean;
  outcome_mode: SchedulerOutcomeMode;
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
  /**
   * Workspace-admin gate for the "Create new" path. Definition CRUD needs
   * workspace ADMIN server-side; without it the modal offers only
   * "Install existing" plus a hint, so nobody sees a form they can't submit.
   */
  canCreateScheduler: boolean;
  onInstalled: (binding: ISchedulerBinding) => void;
  /** Revalidate the workspace scheduler catalog after a definition is created. */
  onSchedulersChanged: () => void;
};

const DEFAULT_VALUES = (): NewSchedulerFormValues => ({
  scheduler: "",
  name: "",
  slug: "",
  description: "",
  prompt: "",
  color: COLOR_PALETTE[0],
  dtstart: defaultDtstartLocal(),
  tzid: Intl.DateTimeFormat().resolvedOptions().timeZone || DEFAULT_TZID,
  rrule: "FREQ=DAILY",
  extra_context: "",
  enabled: true,
  outcome_mode: DEFAULT_OUTCOME_MODE,
  pod: "",
});

const schedulerService = new SchedulerService();

/**
 * Two-path "New Scheduler" modal on the project Schedulers page:
 *
 * - **Install existing** — pick an enabled, not-yet-bound workspace
 *   scheduler and install it on this project (the pre-existing flow).
 * - **Create new** — author a definition (workspace catalog) and install it
 *   on this project in one submit. Two sequential API calls; if the binding
 *   call fails after the definition succeeded, the modal says so, refreshes
 *   the catalog, and flips to "Install existing" with the new definition
 *   preselected so a retry can't orphan it.
 */
export const NewSchedulerModal = observer(function NewSchedulerModal(props: Props) {
  const {
    isOpen,
    onClose,
    workspaceSlug,
    projectId,
    availableSchedulers,
    existingBindings,
    canCreateScheduler,
    onInstalled,
    onSchedulersChanged,
  } = props;
  const { t } = useTranslation();

  const {
    control,
    handleSubmit,
    reset,
    setError,
    setValue,
    formState: { errors, isSubmitting, dirtyFields },
  } = useForm<NewSchedulerFormValues>({ defaultValues: DEFAULT_VALUES() });

  const [mode, setMode] = useState<NewSchedulerMode>("install");
  // RHF keeps inactive-tab fields registered, so their validate closures run
  // on every submit; they read the current mode through this ref to no-op.
  const modeRef = useRef<NewSchedulerMode>(mode);
  modeRef.current = mode;

  const installable = useMemo(() => {
    const boundIds = new Set(existingBindings.map((b) => b.scheduler));
    return availableSchedulers.filter((s) => s.is_enabled && !boundIds.has(s.id));
  }, [availableSchedulers, existingBindings]);

  // SWR may revalidate workspaceSchedulers while the modal is open; only
  // seed on the closed→open edge so the user's in-progress edit isn't wiped.
  const wasOpen = useRef(false);
  useEffect(() => {
    if (isOpen && !wasOpen.current) {
      reset({
        ...DEFAULT_VALUES(),
        scheduler: installable[0]?.id ?? "",
      });
      // Default to the create path when there's nothing left to install —
      // this replaces the old dead-end "No schedulers available" modal.
      setMode(installable.length > 0 || !canCreateScheduler ? "install" : "create");
    }
    wasOpen.current = isOpen;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isOpen, reset]);

  // Auto-derive the slug from the name until the user edits the slug field
  // themselves (programmatic setValue doesn't mark it dirty, typing does).
  const watchedName = useWatch({ control, name: "name" }) ?? "";
  useEffect(() => {
    if (mode !== "create" || dirtyFields.slug) return;
    setValue("slug", deriveSchedulerSlug(watchedName));
  }, [watchedName, mode, dirtyFields.slug, setValue]);

  const watchedDtstart = useWatch({ control, name: "dtstart" }) ?? "";
  const watchedRrule = useWatch({ control, name: "rrule" }) ?? "";

  const handleFormSubmit: SubmitHandler<NewSchedulerFormValues> = async (values) => {
    const bindingPayload = (schedulerId: string) => ({
      scheduler: schedulerId,
      project: projectId,
      dtstart: localToIsoUTC(values.dtstart),
      tzid: values.tzid.trim() || DEFAULT_TZID,
      rrule: values.rrule.trim(),
      extra_context: values.extra_context.trim(),
      enabled: values.enabled,
      outcome_mode: values.outcome_mode,
      pod: values.pod || null,
    });

    if (mode === "create") {
      let created: IScheduler;
      try {
        created = await schedulerService.createScheduler(workspaceSlug, {
          slug: values.slug.trim(),
          name: values.name.trim(),
          description: values.description.trim(),
          prompt: values.prompt,
          color: values.color,
          is_enabled: true,
        });
      } catch (e: unknown) {
        const err = e as { error?: string; slug?: string[]; name?: string[]; prompt?: string[] } | null;
        // Slug uniqueness is per workspace — surface the serializer's 400
        // inline on the field instead of a toast.
        if (err?.slug?.[0]) {
          setError("slug", { type: "server", message: err.slug[0] });
          return;
        }
        setToast({
          type: TOAST_TYPE.ERROR,
          title: t("Something went wrong"),
          message: err?.error ?? err?.name?.[0] ?? err?.prompt?.[0] ?? t("Could not create the scheduler."),
        });
        return;
      }

      // The definition now exists in the workspace catalog either way.
      onSchedulersChanged();

      try {
        const binding = await schedulerService.createBinding(workspaceSlug, projectId, bindingPayload(created.id));
        setToast({
          type: TOAST_TYPE.SUCCESS,
          title: t("Scheduler created and installed"),
          message: t("It will fire on the configured schedule."),
        });
        onInstalled(binding);
        onClose();
      } catch {
        // Don't silently orphan the definition: say what happened and flip
        // to the install path with the fresh definition preselected.
        setToast({
          type: TOAST_TYPE.ERROR,
          title: t("Scheduler created but not installed"),
          message: t("The definition is in the workspace catalog — install it from the “Install existing” tab."),
        });
        setMode("install");
        setValue("scheduler", created.id);
      }
      return;
    }

    try {
      const binding = await schedulerService.createBinding(workspaceSlug, projectId, bindingPayload(values.scheduler));
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

  // Dead end only when there's nothing to install AND the user can't author
  // a new definition (not a workspace admin).
  if (installable.length === 0 && !canCreateScheduler) {
    return (
      <ModalCore isOpen={isOpen} handleClose={onClose} position={EModalPosition.CENTER} width={EModalWidth.XL}>
        <div className="flex flex-col gap-4 p-5">
          <div className="text-18 font-medium text-primary">{t("No schedulers available")}</div>
          <p className="text-13 text-secondary">
            {t(
              "Either every workspace scheduler is already installed on this project, or none are enabled. Ask a workspace admin to add schedulers to the catalog."
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

  const installEmpty = installable.length === 0;
  const submitDisabled = isSubmitting || (mode === "install" && installEmpty);

  return (
    <ModalCore isOpen={isOpen} handleClose={onClose} position={EModalPosition.CENTER} width={EModalWidth.XXL}>
      <form onSubmit={handleSubmit(handleFormSubmit)} className="flex flex-col gap-5 p-5">
        <div className="text-18 font-medium text-primary">{t("New Scheduler")}</div>

        {canCreateScheduler && (
          <div role="tablist" className="flex w-fit items-center gap-1 rounded-md bg-layer-1 p-1">
            <ModeTab label={t("Install existing")} active={mode === "install"} onSelect={() => setMode("install")} />
            <ModeTab label={t("Create new")} active={mode === "create"} onSelect={() => setMode("create")} />
          </div>
        )}

        {mode === "install" &&
          (installEmpty ? (
            <p className="text-13 text-secondary">
              {t("Every enabled workspace scheduler is already installed on this project. Create a new one instead.")}
            </p>
          ) : (
            <div className="flex flex-col gap-1">
              <label htmlFor="binding-scheduler" className="text-13 font-medium text-primary">
                {t("Scheduler")}
              </label>
              <Controller
                control={control}
                name="scheduler"
                rules={{ validate: (v) => modeRef.current !== "install" || !!v || t("Pick a scheduler.") }}
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
                {!canCreateScheduler && <> {t("Ask a workspace admin to add schedulers to the catalog.")}</>}
              </p>
              {errors.scheduler && <span className="text-12 text-danger-primary">{errors.scheduler.message}</span>}
            </div>
          ))}

        {mode === "create" && (
          <SchedulerDefinitionFields
            control={control}
            errors={errors}
            nameName="name"
            slugName="slug"
            descriptionName="description"
            promptName="prompt"
            colorName="color"
            isActive={() => modeRef.current === "create"}
          />
        )}

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

        <div className="flex justify-end gap-2">
          <Button variant="secondary" onClick={onClose} disabled={isSubmitting}>
            {t("Cancel")}
          </Button>
          <Button type="submit" loading={isSubmitting} disabled={submitDisabled}>
            {mode === "create"
              ? isSubmitting
                ? t("Creating & installing…")
                : t("Create & install")
              : isSubmitting
                ? t("Installing…")
                : t("Install")}
          </Button>
        </div>
      </form>
    </ModalCore>
  );
});

type ModeTabProps = {
  label: string;
  active: boolean;
  onSelect: () => void;
};

function ModeTab({ label, active, onSelect }: ModeTabProps) {
  return (
    <button
      type="button"
      role="tab"
      aria-selected={active}
      onClick={onSelect}
      className={`rounded px-3 py-1 text-13 font-medium transition-colors ${
        active ? "shadow-sm bg-surface-1 text-primary" : "text-secondary hover:text-primary"
      }`}
    >
      {label}
    </button>
  );
}
