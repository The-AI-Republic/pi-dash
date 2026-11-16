// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Automatic project-upkeep tab (parity/specs/profile.md §11, PROF-024).
// An instance with nothing scheduled explains itself instead of showing
// dead switches. Otherwise a master pause sits above one card per job;
// pausing never changes a job's own switch, it only makes it read-only
// until the master resumes. Both switches flip on touch and roll back with
// a notice when their write fails. No polling or shortcuts (PROF-027);
// every write is the member's own (PROF-028).

import { Skeleton, Switch } from "@pidash/kit";
import * as React from "react";

import { useSetUpkeepJobEnabled, useSetUpkeepPaused, useUpkeepSettings } from "../api.js";
import {
  MASTER_PAUSE_DESCRIPTION,
  MASTER_PAUSE_TITLE,
  UPKEEP_EMPTY_EXPLANATION,
  UPKEEP_TAB_HEADING,
  UPKEEP_TAB_INTRO,
} from "../lib/copy.js";
import { jobSwitchDisabled, showsEmptyExplanation } from "../lib/rules.js";

/** The quiet frame the tab shows until the member's switches land. */
export function UpkeepSettingsLoading(): React.ReactElement {
  return (
    <div data-testid="upkeep-settings-loading" aria-busy="true" className="flex max-w-2xl flex-col gap-(--space-3)">
      <Skeleton className="h-5 w-56" />
      <Skeleton className="h-4 w-full" />
      <Skeleton className="h-16 w-full" />
    </div>
  );
}

export function UpkeepSettingsPage(): React.ReactElement {
  const settings = useUpkeepSettings();
  const setPaused = useSetUpkeepPaused();
  const setJob = useSetUpkeepJobEnabled();

  if (settings.data === undefined) return <UpkeepSettingsLoading />;

  const data = settings.data;

  return (
    <div data-testid="upkeep-settings-page" className="flex max-w-2xl flex-col gap-(--space-6)">
      <header className="flex flex-col gap-(--space-1)">
        <h2 className="text-h3 text-(--text)">{UPKEEP_TAB_HEADING}</h2>
        <p className="text-body text-(--text-muted)">{UPKEEP_TAB_INTRO}</p>
      </header>

      {showsEmptyExplanation(data) ? (
        <p className="text-body text-(--text-muted)">{UPKEEP_EMPTY_EXPLANATION}</p>
      ) : (
        <>
          <div className="flex items-center justify-between gap-(--space-4) rounded-(--radius-control) border border-(--border) bg-(--surface) px-(--space-4) py-(--space-3)">
            <div className="flex min-w-0 flex-col gap-(--space-1)">
              <span className="text-body font-medium text-(--text)">{MASTER_PAUSE_TITLE}</span>
              <span className="text-meta text-(--text-muted)">{MASTER_PAUSE_DESCRIPTION}</span>
            </div>
            <Switch
              checked={!data.enabled}
              label={MASTER_PAUSE_TITLE}
              data-testid="upkeep-master-switch"
              onCheckedChange={(paused) => setPaused.mutate(paused)}
            />
          </div>

          <ul data-testid="upkeep-job-list" className="flex flex-col gap-(--space-3)">
            {data.jobs.map((job) => (
              <li
                key={job.slug}
                data-testid="upkeep-job"
                data-job-slug={job.slug}
                className="flex items-start justify-between gap-(--space-4) rounded-(--radius-control) border border-(--border) bg-(--surface) px-(--space-4) py-(--space-3)"
              >
                <div className="flex min-w-0 flex-col gap-(--space-1)">
                  <div className="flex items-center gap-(--space-2)">
                    <span className="text-body font-medium text-(--text)">{job.name}</span>
                    <span className="text-caption rounded-(--radius-control) bg-(--subtle) px-(--space-2) py-px text-(--text-muted)">
                      {job.interval_label}
                    </span>
                  </div>
                  <span className="text-meta text-(--text-muted)">{job.description}</span>
                </div>
                <Switch
                  checked={job.enabled}
                  disabled={jobSwitchDisabled(data)}
                  label={job.name}
                  data-testid={`upkeep-job-switch-${job.slug}`}
                  onCheckedChange={(enabled) => setJob.mutate({ slug: job.slug, enabled })}
                />
              </li>
            ))}
          </ul>
        </>
      )}
    </div>
  );
}
