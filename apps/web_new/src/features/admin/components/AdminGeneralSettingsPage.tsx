// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// General settings page (ADM-013–017, from parity/specs/admin.md §2):
// the query wiring around the form. The bootstrap carries the name,
// the instance ID, and the telemetry flag; the admins list carries
// the read-only email. A loader covers the pending reads and an error
// block with a retry covers failed ones.
import { Button, Spinner } from "@pidash/kit";
import * as React from "react";

import { useDocumentHead } from "../../../shared/shell/index.js";
import { firstAdminEmail } from "../generalSettings.js";
import { useInstanceAdmins, useInstanceBootstrap } from "../hooks.js";
import { AdminGeneralSettingsForm } from "./AdminGeneralSettingsForm.js";

export const GENERAL_SETTINGS_TITLE = "General settings — Pi Dash";

export function AdminGeneralSettingsPage(): React.ReactElement {
  useDocumentHead({ title: GENERAL_SETTINGS_TITLE, robots: "noindex, nofollow" });
  const bootstrap = useInstanceBootstrap();
  const admins = useInstanceAdmins();
  const instance = bootstrap.data?.instance;

  const retry = (): void => {
    void bootstrap.refetch();
    void admins.refetch();
  };

  let body: React.ReactNode;
  if (bootstrap.isError || admins.isError) {
    body = (
      <div className="flex flex-col items-start gap-(--space-3)">
        <p role="alert" className="text-body text-(--text)">
          General settings failed to load.
        </p>
        <Button variant="secondary" onClick={retry} data-testid="admin-general-retry">
          Retry
        </Button>
      </div>
    );
  } else if (instance === undefined) {
    body = <Spinner label="Loading general settings" />;
  } else {
    body = <AdminGeneralSettingsForm instance={instance} adminEmail={firstAdminEmail(admins.data)} />;
  }

  return (
    <section data-testid="admin-general" className="flex flex-col gap-(--space-6) p-(--space-8)">
      <div className="flex flex-col gap-(--space-1)">
        <h1 className="text-h2 text-(--text)">General</h1>
        <p className="text-body text-(--text-muted)">Rename this instance and control anonymous telemetry.</p>
      </div>
      {body}
    </section>
  );
}
