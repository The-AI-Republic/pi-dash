// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Generic credential dialog (written from parity/specs/cloud.md §6,
// CLOUD-027): fields built from the server's connection manifest with
// secrets masked and format hints checked, publisher instructions as
// plain text, the issuance link only for safely absolute HTTPS, and
// inline blocking. Submit posts trimmed values and clears secrets;
// success closes, failure stays open with the rejection.
import type { OpenhubApp, OpenhubAuthStatus } from "@pidash/api-client";
import { Button, Dialog, Input } from "@pidash/kit";
import * as React from "react";

import {
  cleanInstructions,
  dialogFields,
  isSafeSetupUrl,
  isSecretField,
  trimCredentialValues,
  validateCredentialValues,
} from "../lib/connect.js";

/** Dialog title naming the app. */
export function connectDialogTitle(appName: string): string {
  return `Connect ${appName}`;
}

export const CONNECT_DIALOG_SUBMIT = "Connect";
export const CONNECT_DIALOG_CANCEL = "Cancel";
export const CONNECT_DIALOG_SETUP_LINK = "Get credentials";

export interface ConnectDialogProps {
  app: OpenhubApp;
  auth: OpenhubAuthStatus;
  /** Post trimmed values; true closes the dialog, false keeps it open. */
  onSubmit: (fields: Record<string, string>) => Promise<boolean>;
  onClose: () => void;
}

export function ConnectDialog({ app, auth, onSubmit, onClose }: ConnectDialogProps): React.ReactElement {
  const fields = dialogFields(auth);
  const [values, setValues] = React.useState<Record<string, string>>({});
  const [attempted, setAttempted] = React.useState(false);
  const [submitting, setSubmitting] = React.useState(false);
  // Secrets clear at submit start; their required-errors stay hidden
  // while the request flies, so the outcome toast owns that moment.
  const problems = attempted && !submitting ? validateCredentialValues(fields, values) : {};
  const instructions = cleanInstructions(auth.manualSetup?.instructions_md);
  const setupUrl = auth.manualSetup?.setupUrl;
  const setupHref = isSafeSetupUrl(setupUrl) ? setupUrl : null;

  const submit = (): void => {
    if (submitting) return;
    setAttempted(true);
    if (Object.keys(validateCredentialValues(fields, values)).length > 0) return;
    setSubmitting(true);
    void (async () => {
      try {
        const trimmed = trimCredentialValues(fields, values);
        // Secrets leave memory on every submit; other values stay, so a
        // rejection only costs retyping the secret.
        setValues((previous) => {
          const next = { ...previous };
          for (const field of fields) {
            if (isSecretField(field)) next[field.key] = "";
          }
          return next;
        });
        const ok = await onSubmit(trimmed);
        if (ok) onClose();
      } finally {
        setSubmitting(false);
      }
    })();
  };

  return (
    <Dialog
      title={connectDialogTitle(app.name)}
      open
      onOpenChange={() => onClose()}
      actions={
        <>
          <Button variant="secondary" onClick={onClose}>
            {CONNECT_DIALOG_CANCEL}
          </Button>
          <Button variant="primary" loading={submitting} onClick={submit}>
            {CONNECT_DIALOG_SUBMIT}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-(--space-4)">
        {instructions === "" ? null : <p className="text-body text-(--text-muted)">{instructions}</p>}
        {setupHref === null ? null : (
          <a href={setupHref} target="_blank" rel="noreferrer" className="font-medium text-(--accent) hover:underline">
            {CONNECT_DIALOG_SETUP_LINK}
          </a>
        )}
        {fields.map((field) => {
          const problem = problems[field.key];
          return (
            <Input
              key={field.key}
              label={field.label}
              type={isSecretField(field) ? "password" : "text"}
              autoComplete="off"
              value={values[field.key] ?? ""}
              onChange={(event) => setValues((previous) => ({ ...previous, [field.key]: event.target.value }))}
              {...(field.placeholder === undefined ? {} : { placeholder: field.placeholder })}
              {...(problem === undefined ? {} : { error: problem })}
            />
          );
        })}
      </div>
    </Dialog>
  );
}
