/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import useSWR from "swr";
// pi dash imports
import { useTranslation } from "@pi-dash/i18n";
import { setToast, TOAST_TYPE } from "@pi-dash/propel/toast";
import { AutoPMService } from "@pi-dash/services";
import type { IAutoPMJob, IAutoPMSettings } from "@pi-dash/types";
import { ToggleSwitch } from "@pi-dash/ui";

const service = new AutoPMService();

export const AutoProjectManagementSettings = observer(function AutoProjectManagementSettings() {
  const { t } = useTranslation();
  const { data, mutate, isLoading } = useSWR<IAutoPMSettings>("auto-pm-settings", () => service.getSettings());

  const masterEnabled = data?.enabled ?? true;
  const jobs = data?.jobs ?? [];

  const onError = (message?: string) =>
    setToast({ type: TOAST_TYPE.ERROR, title: t("Something went wrong"), message: message || t("Please try again.") });

  const toggleMaster = async (next: boolean) => {
    // Optimistic: flip immediately, revalidate from the server response.
    void mutate({ enabled: next, jobs }, false);
    try {
      const fresh = await service.setMasterEnabled(next);
      void mutate(fresh, false);
    } catch (e: unknown) {
      const err = e as { error?: string } | null;
      onError(err?.error);
      void mutate();
    }
  };

  const toggleJob = async (job: IAutoPMJob, next: boolean) => {
    void mutate(
      { enabled: masterEnabled, jobs: jobs.map((j) => (j.slug === job.slug ? { ...j, enabled: next } : j)) },
      false
    );
    try {
      const fresh = await service.setJobEnabled(job.slug, next);
      void mutate(fresh, false);
    } catch (e: unknown) {
      const err = e as { error?: string } | null;
      onError(err?.error);
      void mutate();
    }
  };

  return (
    <div className="flex max-w-xl flex-col gap-5">
      <div>
        <h3 className="text-16 font-semibold text-primary">{t("Auto Project Management")}</h3>
        <p className="mt-1 text-13 text-secondary">
          {t(
            "Pi Dash AI can do routine project upkeep for you automatically. It acts with your permissions and only in workspaces you belong to."
          )}
        </p>
      </div>

      {!isLoading && jobs.length === 0 ? (
        <p className="text-13 text-secondary">{t("Nothing is scheduled on this instance yet.")}</p>
      ) : (
        <>
          <div className="flex items-center justify-between rounded-md border border-subtle bg-surface-1 px-4 py-3">
            <div>
              <div className="text-14 font-medium text-primary">{t("Pause all Auto Project Management")}</div>
              <div className="text-12 text-secondary">{t("Turn everything off without changing individual jobs.")}</div>
            </div>
            <ToggleSwitch value={!masterEnabled} onChange={(paused) => toggleMaster(!paused)} size="sm" />
          </div>

          <div className="flex flex-col gap-3">
            {jobs.map((job) => (
              <div
                key={job.slug}
                className="flex items-start justify-between gap-4 rounded-md border border-subtle bg-surface-1 px-4 py-3"
              >
                <div className="flex flex-col gap-0.5">
                  <div className="flex items-center gap-2">
                    <span className="text-14 font-medium text-primary">{job.name}</span>
                    <span className="rounded bg-surface-2 px-1.5 py-0.5 text-11 text-secondary capitalize">
                      {t(job.interval_label)}
                    </span>
                  </div>
                  <span className="text-12 text-secondary">{job.description}</span>
                </div>
                <ToggleSwitch
                  value={job.enabled}
                  onChange={(next) => toggleJob(job, next)}
                  disabled={!masterEnabled}
                  size="sm"
                />
              </div>
            ))}
          </div>
        </>
      )}
    </div>
  );
});
