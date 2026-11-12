// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Email preferences page content (parity/specs/notifications.md §10,
// NTF-024/025/031). A loader stands in while the settings load; then one
// toggle per email topic appears, each saving instantly on flip, with the
// completed-only refinement nested under state changes. The toggles are
// controlled by the preferences query, so fresh settings re-sync the form
// and a failed save springs the toggle back to the persisted value. A
// failed load reports a short error with no retry control, keeping
// failure feedback lightweight (NTF-031).
import { Spinner, Switch } from "@pidash/kit";
import * as React from "react";

import { useEmailPreferences, useUpdateEmailPreference, type EmailPreferenceTopic } from "../api/preferences.js";

export const EMAIL_PREFS_HEADING = "Email notifications";
export const EMAIL_PREFS_INTRO = "Choose which emails you receive about your work. Each change saves immediately.";
export const EMAIL_PREFS_LOADING_LABEL = "Loading email preferences";
export const EMAIL_PREFS_LOAD_FAILED = "Email preferences could not be loaded.";

/** One top-level toggle row: the topic key and its heading. */
export interface EmailPreferenceRow {
  topic: EmailPreferenceTopic;
  heading: string;
}

/** Top-level topics in display order. */
export const EMAIL_PREFERENCE_ROWS: EmailPreferenceRow[] = [
  { topic: "property_change", heading: "Property changes" },
  { topic: "state_change", heading: "State changes" },
  { topic: "comment", heading: "Comments" },
  { topic: "mention", heading: "Mentions" },
];

/** The completed-only refinement, nested under state changes. */
export const COMPLETED_ONLY_ROW: EmailPreferenceRow = {
  topic: "issue_completed",
  heading: "Completed items only",
};

export interface PendingFlip {
  isPending: boolean;
  variables: { topic: EmailPreferenceTopic } | undefined;
}

/** The topic with a save in flight, if any — its toggle stays disabled. */
export function pendingFlipTopic(mutation: PendingFlip): EmailPreferenceTopic | undefined {
  if (!mutation.isPending || mutation.variables === undefined) return undefined;
  return mutation.variables.topic;
}

interface PreferenceRowProps {
  row: EmailPreferenceRow;
  checked: boolean;
  disabled: boolean;
  onFlip: (topic: EmailPreferenceTopic, enabled: boolean) => void;
}

function PreferenceRow({ row, checked, disabled, onFlip }: PreferenceRowProps): React.ReactElement {
  return (
    <div className="flex items-center justify-between gap-(--space-4) py-(--space-3)">
      <h2 className="text-emphasis text-(--text)">{row.heading}</h2>
      <Switch
        label={row.heading}
        checked={checked}
        disabled={disabled}
        onCheckedChange={(enabled) => onFlip(row.topic, enabled)}
      />
    </div>
  );
}

export function EmailPreferencesPage(): React.ReactElement {
  const prefs = useEmailPreferences();
  const update = useUpdateEmailPreference();

  if (prefs.isPending) {
    return (
      <div data-testid="email-prefs-loader" className="grid place-items-center py-(--space-8)">
        <Spinner label={EMAIL_PREFS_LOADING_LABEL} />
      </div>
    );
  }
  if (prefs.isError) {
    return (
      <p role="alert" className="text-body text-(--text)">
        {EMAIL_PREFS_LOAD_FAILED}
      </p>
    );
  }

  const values = prefs.data;
  const pendingTopic = pendingFlipTopic(update);
  const flip = (topic: EmailPreferenceTopic, enabled: boolean) => {
    update.mutate({ topic, enabled });
  };
  const rowProps = (row: EmailPreferenceRow) => ({
    checked: values[row.topic],
    disabled: pendingTopic === row.topic,
  });

  return (
    <section aria-labelledby="email-prefs-heading">
      <h1 id="email-prefs-heading" className="text-emphasis text-(--text)">
        {EMAIL_PREFS_HEADING}
      </h1>
      <p className="text-body text-(--text-muted)">{EMAIL_PREFS_INTRO}</p>
      <div data-testid="email-prefs-form" role="group" aria-label={EMAIL_PREFS_HEADING}>
        {EMAIL_PREFERENCE_ROWS.map((row) => (
          <React.Fragment key={row.topic}>
            <PreferenceRow row={row} {...rowProps(row)} onFlip={flip} />
            {row.topic === "state_change" && (
              <div data-testid="email-prefs-completed-nest" className="ml-(--space-6)">
                <PreferenceRow row={COMPLETED_ONLY_ROW} {...rowProps(COMPLETED_ONLY_ROW)} onFlip={flip} />
              </div>
            )}
          </React.Fragment>
        ))}
      </div>
    </section>
  );
}
