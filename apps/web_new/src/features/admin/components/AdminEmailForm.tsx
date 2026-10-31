// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// SMTP configuration form (ADM-019/020, from parity/specs/admin.md
// §3.2-3.3): required host, port, and sender address plus an optional
// username/password section with show/hide and the TLS/SSL/none
// transport-security choice. Save stays shut unless the form is valid
// and dirty (the security picker dirties it like any other control,
// NEWFRONT-478 intent) and forces SMTP back on. The test-email action
// opens from any valid form, saved or not, and hands the modal the
// values on screen so the check grades what the admin entered
// (NEWFRONT-477).
import { Button, Input } from "@pidash/kit";
import { Eye, EyeOff } from "lucide-react";
import * as React from "react";

import {
  buildSmtpPatch,
  emailFormValuesFromConfig,
  isEmailFormDirty,
  isEmailFormValid,
  type EmailFormValues,
  type EmailSecurityChoice,
} from "../emailConfig.js";
import { configValueMap, useSaveSmtpConfig } from "../hooks.js";
import { AdminTestEmailModal } from "./AdminTestEmailModal.js";

/** Idle save label (wording pinned by the parity scenario). */
export const SMTP_SAVE_LABEL = "Save changes";
/** In-flight save label (wording pinned by the parity scenario). */
export const SMTP_SAVING_LABEL = "Saving";

const SECURITY_CHOICES: ReadonlyArray<{ choice: EmailSecurityChoice; label: string; testId: string }> = [
  { choice: "tls", label: "TLS", testId: "admin-email-security-tls" },
  { choice: "ssl", label: "SSL", testId: "admin-email-security-ssl" },
  { choice: "none", label: "None", testId: "admin-email-security-none" },
];

export interface AdminEmailFormProps {
  /** Stored values the form edits; seeds the mount-time baseline. */
  initial: EmailFormValues;
  /** Runs after a save lands, so the page clears its toggle override. */
  onSaved: () => void;
}

export function AdminEmailForm({ initial, onSaved }: AdminEmailFormProps): React.ReactElement {
  const [values, setValues] = React.useState(initial);
  // The saved values dirtiness is judged against: reseeded from every
  // save response, so the form turns pristine exactly when the server
  // holds what it shows.
  const [baseline, setBaseline] = React.useState(initial);
  const [showPassword, setShowPassword] = React.useState(false);
  const [modalOpen, setModalOpen] = React.useState(false);
  const save = useSaveSmtpConfig();

  const valid = isEmailFormValid(values);
  const dirty = isEmailFormDirty(values, baseline);

  const patch = (edit: Partial<EmailFormValues>) => {
    setValues((current) => ({ ...current, ...edit }));
  };

  const persist = () => {
    if (!valid || !dirty || save.isPending) return;
    save.mutate(buildSmtpPatch(values), {
      onSuccess: (rows) => {
        const saved = emailFormValuesFromConfig(configValueMap(rows));
        setValues(saved);
        setBaseline(saved);
        onSaved();
      },
    });
  };

  return (
    <>
      <form
        data-testid="admin-email-form"
        className="flex flex-col gap-(--space-6)"
        onSubmit={(event) => {
          event.preventDefault();
          persist();
        }}
      >
        <section aria-label="SMTP server" className="flex flex-col gap-(--space-4)">
          <Input
            label="SMTP host"
            id="admin-email-host"
            type="text"
            autoComplete="off"
            required
            value={values.host}
            onChange={(event) => patch({ host: event.target.value })}
            data-testid="admin-email-host"
          />
          <Input
            label="Port"
            id="admin-email-port"
            type="text"
            autoComplete="off"
            required
            value={values.port}
            onChange={(event) => patch({ port: event.target.value })}
            data-testid="admin-email-port"
          />
          <Input
            label="Sender address"
            id="admin-email-from"
            type="text"
            autoComplete="email"
            required
            value={values.from}
            onChange={(event) => patch({ from: event.target.value })}
            data-testid="admin-email-from"
          />
        </section>
        <section aria-label="SMTP authentication" className="flex flex-col gap-(--space-4)">
          <Input
            label="Username"
            hint="Optional"
            id="admin-email-user"
            type="text"
            autoComplete="off"
            value={values.user}
            onChange={(event) => patch({ user: event.target.value })}
            data-testid="admin-email-user"
          />
          <Input
            label="Password"
            hint="Optional"
            id="admin-email-password"
            type={showPassword ? "text" : "password"}
            autoComplete="off"
            value={values.password}
            onChange={(event) => patch({ password: event.target.value })}
            trailing={
              <button
                type="button"
                tabIndex={-1}
                data-testid="admin-email-toggle-password"
                aria-label={showPassword ? "Hide password" : "Show password"}
                onClick={() => setShowPassword((current) => !current)}
                className="text-body cursor-pointer text-(--text-muted) hover:text-(--text)"
              >
                {showPassword ? <EyeOff size={16} aria-hidden="true" /> : <Eye size={16} aria-hidden="true" />}
              </button>
            }
            data-testid="admin-email-password"
          />
        </section>
        <fieldset className="flex flex-col gap-(--space-2)">
          <legend className="text-emphasis font-medium text-(--text)">Connection security</legend>
          <div className="flex items-center gap-(--space-6)">
            {SECURITY_CHOICES.map((entry) => (
              <label
                key={entry.choice}
                className="text-body flex cursor-pointer items-center gap-(--space-2) text-(--text)"
              >
                <input
                  type="radio"
                  name="admin-email-security"
                  value={entry.choice}
                  checked={values.security === entry.choice}
                  onChange={() => patch({ security: entry.choice })}
                  data-testid={entry.testId}
                  className="size-4 shrink-0 accent-(--accent)"
                />
                {entry.label}
              </label>
            ))}
          </div>
        </fieldset>
        <div className="flex items-center gap-(--space-4)">
          <Button
            variant="primary"
            onClick={persist}
            loading={save.isPending}
            disabled={!valid || !dirty || save.isPending}
            data-testid="admin-email-save"
          >
            {save.isPending ? SMTP_SAVING_LABEL : SMTP_SAVE_LABEL}
          </Button>
          <Button
            variant="secondary"
            onClick={() => setModalOpen(true)}
            disabled={!valid}
            data-testid="admin-email-test-email"
          >
            Send test email
          </Button>
        </div>
      </form>
      {modalOpen ? <AdminTestEmailModal candidate={values} onClose={() => setModalOpen(false)} /> : null}
    </>
  );
}
