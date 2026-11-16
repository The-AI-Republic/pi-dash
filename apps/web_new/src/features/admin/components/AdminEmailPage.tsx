// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Email settings page (ADM-018/022, from parity/specs/admin.md §3.1 and
// §3.5): the header carries the master enable toggle, the body the SMTP
// form. The toggle derives from the stored flag; turning off DELETEs the
// feature (the form hides at once through the optimistic blanking) and
// turning on reveals the form with no server call (a local override until
// the next save lands). While configurations load, a pill skeleton stands
// in for the toggle; with no configuration behind an on toggle, a
// five-row skeleton stands in for the form. Skeletons never render
// beside the real controls.
import { Skeleton, Switch } from "@pidash/kit";
import * as React from "react";

import { useDocumentHead } from "../../../shared/shell/index.js";
import { emailFormValuesFromConfig, parseConfigFlag, SMTP_ENABLED_KEY } from "../emailConfig.js";
import { configValueMap, useDisableEmailFeature, useInstanceConfigurations } from "../hooks.js";
import { AdminEmailForm } from "./AdminEmailForm.js";

/** Document title for the email settings route. */
export const EMAIL_PAGE_TITLE = "Email settings — Pi Dash";

/** Form skeleton rows while an on toggle has no configuration (ADM-022). */
const FORM_SKELETON_ROWS = [0, 1, 2, 3, 4];

export function AdminEmailPage(): React.ReactElement {
  useDocumentHead({ title: EMAIL_PAGE_TITLE, robots: "noindex, nofollow" });
  const configurations = useInstanceConfigurations();
  const disable = useDisableEmailFeature();
  const [override, setOverride] = React.useState<boolean | null>(null);

  const rows = configurations.data;
  const storedOn = rows === undefined ? false : parseConfigFlag(configValueMap(rows)[SMTP_ENABLED_KEY]);
  const enabled = override ?? storedOn;
  const loading = configurations.isPending;
  const initial = React.useMemo(
    () => (rows === undefined ? null : emailFormValuesFromConfig(configValueMap(rows))),
    [rows]
  );

  const flip = (checked: boolean) => {
    if (checked) {
      setOverride(true);
      return;
    }
    setOverride(false);
    disable.mutate(undefined, { onSettled: () => setOverride(null) });
  };

  return (
    <div
      data-testid="admin-email-page"
      className="mx-auto flex w-full max-w-160 flex-col gap-(--space-8) p-(--space-8)"
    >
      <div className="flex items-start justify-between gap-(--space-6)">
        <div className="flex flex-col gap-(--space-1)">
          <h1 data-testid="admin-email-heading" className="text-h2 text-(--text)">
            Email
          </h1>
          <p className="text-body text-(--text-muted)">Configure the SMTP server the instance uses to send mail.</p>
        </div>
        {loading ? (
          <div data-testid="admin-email-header-skeleton" className="pt-(--space-1)">
            <Skeleton width="48px" height="28px" round />
          </div>
        ) : (
          <Switch
            checked={enabled}
            onCheckedChange={flip}
            aria-label="Enable email"
            disabled={disable.isPending}
            data-testid="admin-email-toggle"
          />
        )}
      </div>
      {loading || !enabled ? null : initial === null ? (
        <div data-testid="admin-email-form-skeleton" role="status" aria-label="Loading email settings">
          <div className="flex flex-col gap-(--space-4)">
            {FORM_SKELETON_ROWS.map((row) => (
              <div key={row} data-testid="admin-email-form-skeleton-row" className="flex flex-col gap-(--space-2)">
                <Skeleton width="120px" height="12px" />
                <Skeleton height="32px" />
              </div>
            ))}
          </div>
        </div>
      ) : (
        <AdminEmailForm initial={initial} onSaved={() => setOverride(null)} />
      )}
    </div>
  );
}
