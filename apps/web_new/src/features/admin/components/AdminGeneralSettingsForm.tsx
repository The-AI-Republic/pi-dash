// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// General settings form (ADM-013–017, from parity/specs/admin.md §2):
// the required instance name, the read-only admin email and instance
// ID, the telemetry switch with its policy link, and the shared save.
// The switch flips locally and persists only through the save; an
// empty name fails with an inline required message and sends nothing
// (NEWFRONT-475 intent); the read-only pair never rides the payload.
import type { InstanceDetails } from "@pidash/api-client";
import { Button, Input } from "@pidash/kit";
import * as React from "react";

import { useUpdateGeneralSettings } from "../hooks.js";
import {
  GENERAL_NAME_REQUIRED_MESSAGE,
  GENERAL_SAVE_BUSY_LABEL,
  GENERAL_SAVE_IDLE_LABEL,
  GENERAL_TELEMETRY_DOC_URL,
  isGeneralNameValid,
  resolveTelemetryPrefill,
} from "../generalSettings.js";

export function AdminGeneralSettingsForm({
  instance,
  adminEmail,
}: {
  instance: InstanceDetails;
  adminEmail: string;
}): React.ReactElement {
  const [name, setName] = React.useState(instance.instance_name);
  const [telemetry, setTelemetry] = React.useState(resolveTelemetryPrefill(instance.is_telemetry_enabled));
  const [saveAttempted, setSaveAttempted] = React.useState(false);
  const saving = useUpdateGeneralSettings();
  const nameError = saveAttempted && !isGeneralNameValid(name) ? GENERAL_NAME_REQUIRED_MESSAGE : undefined;

  const onSubmit = (event: React.FormEvent<HTMLFormElement>): void => {
    event.preventDefault();
    setSaveAttempted(true);
    if (!isGeneralNameValid(name)) return;
    saving.mutate({ instance_name: name, is_telemetry_enabled: telemetry });
  };

  return (
    <form
      data-testid="admin-general-form"
      onSubmit={onSubmit}
      className="flex w-full max-w-140 flex-col gap-(--space-6)"
    >
      <Input
        label="Instance name"
        value={name}
        onChange={(event) => setName(event.target.value)}
        {...(nameError === undefined ? {} : { error: nameError })}
        data-testid="admin-general-name"
      />
      <Input label="Administrator email" value={adminEmail} disabled data-testid="admin-general-email" />
      <Input label="Instance ID" value={instance.instance_id} disabled data-testid="admin-general-instance-id" />
      <div className="flex items-start justify-between gap-(--space-4)">
        <div className="flex flex-col gap-(--space-1)">
          <p id="admin-general-telemetry-label" className="text-body font-medium text-(--text)">
            Usage telemetry
          </p>
          <p className="text-meta text-(--text-muted)">
            Share anonymous usage statistics to help improve Pi Dash. No personal data is collected.{" "}
            <a
              href={GENERAL_TELEMETRY_DOC_URL}
              target="_blank"
              rel="noopener noreferrer"
              className="text-(--accent) underline"
            >
              Learn more
            </a>
          </p>
        </div>
        <button
          type="button"
          role="switch"
          aria-checked={telemetry}
          aria-labelledby="admin-general-telemetry-label"
          disabled={saving.isPending}
          onClick={() => setTelemetry((current) => !current)}
          data-testid="admin-general-telemetry"
          data-on={telemetry}
          className="relative h-6 w-11 shrink-0 cursor-pointer rounded-(--radius-full) bg-(--border) transition-colors duration-(--motion-duration) disabled:cursor-not-allowed disabled:opacity-50 data-[on=true]:bg-(--accent)"
        >
          <span
            aria-hidden="true"
            data-on={telemetry}
            className="shadow absolute top-0.5 left-0.5 size-5 rounded-full bg-(--bg) transition-transform duration-(--motion-duration) data-[on=true]:translate-x-5"
          />
        </button>
      </div>
      <div>
        <Button type="submit" variant="primary" loading={saving.isPending} data-testid="admin-general-save">
          {saving.isPending ? GENERAL_SAVE_BUSY_LABEL : GENERAL_SAVE_IDLE_LABEL}
        </Button>
      </div>
    </form>
  );
}
