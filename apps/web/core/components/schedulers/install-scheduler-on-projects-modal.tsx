/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useMemo, useRef, useState } from "react";
import { observer } from "mobx-react";
import type { SubmitHandler } from "react-hook-form";
import { useForm, useWatch } from "react-hook-form";
import { useTranslation } from "@pi-dash/i18n";
import { Button } from "@pi-dash/propel/button";
import { Logo } from "@pi-dash/propel/emoji-icon-picker";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import type { IScheduler, SchedulerOutcomeMode } from "@pi-dash/services";
import { SchedulerService } from "@pi-dash/services";
import { Badge, Checkbox, EModalPosition, EModalWidth, Input, Loader, ModalCore } from "@pi-dash/ui";
import {
  BindingOutcomeModeField,
  DEFAULT_OUTCOME_MODE,
} from "@/components/project/scheduler-bindings/binding-outcome-mode-field";
import { BindingScheduleFields } from "@/components/project/scheduler-bindings/binding-schedule-fields";
import { DEFAULT_TZID } from "@/components/project/scheduler-bindings/constants";
import { defaultDtstartLocal, localToIsoUTC } from "@/components/project/scheduler-bindings/datetime-input";
import { filterProjects, partitionInstallResults } from "@/components/schedulers/install-scheduler-helpers";
import { useProject } from "@/hooks/store/use-project";

interface InstallFormValues {
  dtstart: string;
  tzid: string;
  rrule: string;
  extra_context: string;
  enabled: boolean;
  outcome_mode: SchedulerOutcomeMode;
}

type Props = {
  isOpen: boolean;
  onClose: () => void;
  workspaceSlug: string;
  /** Template being installed. Null when the modal is closed. */
  scheduler: IScheduler | null;
  /** Called after at least one binding is created so the caller can refresh install counts. */
  onInstalled: () => void;
};

const DEFAULT_VALUES = (): InstallFormValues => ({
  dtstart: defaultDtstartLocal(),
  tzid: Intl.DateTimeFormat().resolvedOptions().timeZone || DEFAULT_TZID,
  rrule: "FREQ=DAILY",
  extra_context: "",
  enabled: true,
  outcome_mode: DEFAULT_OUTCOME_MODE,
});

const schedulerService = new SchedulerService();

/**
 * Install a workspace scheduler template onto one or more projects in a single
 * pass. The inverse of the project-side install modal: there the project is
 * fixed and you pick a scheduler; here the scheduler is fixed and you pick the
 * projects. Schedule + outcome metadata is shared across every selected
 * project (the per-project edit modal can tweak an install afterwards). Pod is
 * intentionally omitted — it is project-scoped, so each binding defaults to its
 * project's default pod.
 */
export const InstallSchedulerOnProjectsModal = observer(function InstallSchedulerOnProjectsModal(props: Props) {
  const { isOpen, onClose, workspaceSlug, scheduler, onInstalled } = props;
  const { t } = useTranslation();
  const { joinedProjectIds, getProjectById } = useProject();

  const {
    control,
    handleSubmit,
    reset,
    formState: { errors, isSubmitting },
  } = useForm<InstallFormValues>({ defaultValues: DEFAULT_VALUES() });

  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [query, setQuery] = useState("");
  // Project ids that already have THIS scheduler installed. Loaded lazily when
  // the modal opens; until then everything is treated as installable.
  const [installedIds, setInstalledIds] = useState<Set<string> | null>(null);

  const projects = useMemo(
    () => joinedProjectIds.map((id) => getProjectById(id)).filter((p): p is NonNullable<typeof p> => !!p),
    [joinedProjectIds, getProjectById]
  );

  // Seed the form + selection only on the closed→open edge so a mid-edit SWR
  // revalidation upstream can't wipe the user's in-progress choices.
  const wasOpen = useRef(false);
  useEffect(() => {
    if (isOpen && !wasOpen.current) {
      reset(DEFAULT_VALUES());
      setSelected(new Set());
      setQuery("");
      setInstalledIds(null);
    }
    wasOpen.current = isOpen;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isOpen, reset]);

  // Detect already-installed projects by scanning each project's bindings for
  // one pointing at this scheduler. Best-effort: a per-project failure (e.g.
  // permissions) just leaves that project marked installable.
  useEffect(() => {
    if (!isOpen || !scheduler) return;
    let cancelled = false;
    const schedulerId = scheduler.id;
    const ids = joinedProjectIds.slice();
    (async () => {
      const checks = await Promise.all(
        ids.map(async (pid) => {
          try {
            const bindings = await schedulerService.listBindings(workspaceSlug, pid);
            return bindings.some((b) => b.scheduler === schedulerId) ? pid : null;
          } catch {
            return null;
          }
        })
      );
      if (cancelled) return;
      setInstalledIds(new Set(checks.filter((x): x is string => x !== null)));
    })();
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isOpen, scheduler?.id, workspaceSlug, joinedProjectIds]);

  const watchedDtstart = useWatch({ control, name: "dtstart" }) ?? "";
  const watchedRrule = useWatch({ control, name: "rrule" }) ?? "";

  const filteredProjects = useMemo(() => filterProjects(projects, query), [projects, query]);

  const isInstalled = (id: string) => installedIds?.has(id) ?? false;

  // Eligible = visible (post-filter) and not already installed.
  const eligibleFilteredIds = useMemo(
    () => filteredProjects.filter((p) => !isInstalled(p.id)).map((p) => p.id),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [filteredProjects, installedIds]
  );
  const allEligibleSelected = eligibleFilteredIds.length > 0 && eligibleFilteredIds.every((id) => selected.has(id));
  const someEligibleSelected = eligibleFilteredIds.some((id) => selected.has(id));

  const toggleProject = (id: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const toggleSelectAll = () => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (allEligibleSelected) eligibleFilteredIds.forEach((id) => next.delete(id));
      else eligibleFilteredIds.forEach((id) => next.add(id));
      return next;
    });
  };

  const handleFormSubmit: SubmitHandler<InstallFormValues> = async (values) => {
    if (!scheduler) return;
    const targetIds = [...selected];
    if (targetIds.length === 0) {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Select a project"),
        message: t("Pick at least one project to install this scheduler on."),
      });
      return;
    }

    const results = await Promise.allSettled(
      targetIds.map((pid) =>
        schedulerService.createBinding(workspaceSlug, pid, {
          scheduler: scheduler.id,
          project: pid,
          dtstart: localToIsoUTC(values.dtstart),
          tzid: values.tzid.trim() || DEFAULT_TZID,
          rrule: values.rrule.trim(),
          extra_context: values.extra_context.trim(),
          enabled: values.enabled,
          outcome_mode: values.outcome_mode,
        })
      )
    );

    const { succeededIds, failedIds, firstError } = partitionInstallResults(targetIds, results);

    if (succeededIds.length > 0) {
      setToast({
        type: TOAST_TYPE.SUCCESS,
        title: t("{count, plural, one {Installed on # project} other {Installed on # projects}}", {
          count: succeededIds.length,
        }),
        message: t("It will fire on the configured schedule."),
      });
      onInstalled();
    }

    if (failedIds.length > 0) {
      const names = failedIds.map((id) => getProjectById(id)?.name ?? id).join(", ");
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("{count, plural, one {# project failed} other {# projects failed}}", { count: failedIds.length }),
        message: firstError ? `${names} — ${firstError}` : names,
      });
      // Keep the modal open with only the failures still selected so the user
      // can retry them; mark the successful ones as installed.
      setSelected(new Set(failedIds));
      setInstalledIds((prev) => {
        const next = new Set(prev ?? []);
        succeededIds.forEach((id) => next.add(id));
        return next;
      });
      return;
    }

    onClose();
  };

  if (!scheduler) return null;

  const noProjects = projects.length === 0;

  return (
    <ModalCore isOpen={isOpen} handleClose={onClose} position={EModalPosition.CENTER} width={EModalWidth.XXL}>
      <form onSubmit={handleSubmit(handleFormSubmit)} className="flex flex-col gap-5 p-5">
        <div className="flex flex-col gap-1">
          <div className="flex items-center gap-2">
            <span
              className="inline-block h-3 w-3 flex-shrink-0 rounded-sm"
              style={{ backgroundColor: scheduler.color || "#3b82f6" }}
              aria-hidden="true"
            />
            <span className="text-18 font-medium text-primary">
              {t("Install")} {scheduler.name}
            </span>
          </div>
          {scheduler.description && <p className="text-13 text-secondary">{scheduler.description}</p>}
        </div>

        {/* Project picker */}
        <div className="flex flex-col gap-2">
          <div className="flex items-center justify-between gap-2">
            <span className="text-13 font-medium text-primary">{t("Projects")}</span>
            {!noProjects && (
              <button
                type="button"
                onClick={toggleSelectAll}
                disabled={eligibleFilteredIds.length === 0}
                className="text-12 text-accent-primary hover:underline disabled:cursor-not-allowed disabled:text-secondary disabled:no-underline"
              >
                {allEligibleSelected ? t("Clear selection") : t("Select all")}
              </button>
            )}
          </div>

          {noProjects ? (
            <p className="rounded-md border border-subtle px-3 py-6 text-center text-13 text-secondary">
              {t("You aren't a member of any project in this workspace to install on.")}
            </p>
          ) : (
            <>
              <Input
                type="text"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder={t("Search projects…")}
                className="w-full"
              />
              <div className="max-h-64 overflow-y-auto rounded-md border border-subtle">
                {installedIds === null ? (
                  <Loader className="flex flex-col gap-2 p-3">
                    <Loader.Item height="36px" />
                    <Loader.Item height="36px" />
                    <Loader.Item height="36px" />
                  </Loader>
                ) : filteredProjects.length === 0 ? (
                  <p className="px-3 py-6 text-center text-13 text-secondary">{t("No projects match your search.")}</p>
                ) : (
                  <ul>
                    {filteredProjects.map((project) => {
                      const installed = isInstalled(project.id);
                      const checked = selected.has(project.id);
                      return (
                        <li key={project.id} className="border-b border-subtle last:border-b-0">
                          <label
                            className={`flex items-center gap-3 px-3 py-2 ${
                              installed ? "cursor-not-allowed opacity-60" : "cursor-pointer hover:bg-layer-1"
                            }`}
                          >
                            <Checkbox
                              checked={installed || checked}
                              disabled={installed}
                              onChange={() => toggleProject(project.id)}
                            />
                            <span className="flex-shrink-0">
                              <Logo logo={project.logo_props} size={14} />
                            </span>
                            <span className="flex min-w-0 flex-col">
                              <span className="truncate text-13 font-medium text-primary">{project.name}</span>
                              {project.identifier && (
                                <span className="truncate text-12 text-secondary">{project.identifier}</span>
                              )}
                            </span>
                            {installed && (
                              <span className="ml-auto flex-shrink-0">
                                <Badge variant="accent-neutral" size="sm">
                                  {t("Installed")}
                                </Badge>
                              </span>
                            )}
                          </label>
                        </li>
                      );
                    })}
                  </ul>
                )}
              </div>
              <p className="text-12 text-secondary">
                {someEligibleSelected
                  ? t("{count, plural, one {# project selected} other {# projects selected}}", {
                      count: [...selected].filter((id) => !isInstalled(id)).length,
                    })
                  : t("Select the projects to install this scheduler on.")}
              </p>
            </>
          )}
        </div>

        {/* Shared install metadata */}
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

        <div className="flex justify-end gap-2">
          <Button variant="secondary" onClick={onClose} disabled={isSubmitting}>
            {t("Cancel")}
          </Button>
          <Button type="submit" loading={isSubmitting} disabled={isSubmitting || noProjects}>
            {isSubmitting ? t("Installing…") : t("Install")}
          </Button>
        </div>
      </form>
    </ModalCore>
  );
});
