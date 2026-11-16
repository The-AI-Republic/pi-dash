// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared new-password form. The reset-password and set-password pages
// render this unit: the address locked when one is supplied, the secret
// plus confirmation behind the AUTH-010 gates (strength plus match, submit
// disabled until both hold, mismatch message, per-field show/hide),
// and a strength indicator while the secret is weak. Written from
// parity/specs/auth.md AUTH-010 and AUTH-013-014.

import { Button, Input } from "@pidash/kit";
import * as React from "react";

import { canSubmitNewPassword, passwordMismatchError, weakSecretHint } from "../recovery.js";

export interface NewPasswordFormProps {
  /** Locked address display; omit it where no address applies. */
  email?: string | undefined;
  submitLabel: string;
  pending: boolean;
  onSubmit: (password: string) => void;
}

export function NewPasswordForm({ email, submitLabel, pending, onSubmit }: NewPasswordFormProps): React.ReactElement {
  const [password, setPassword] = React.useState("");
  const [confirm, setConfirm] = React.useState("");
  const [confirmBlurred, setConfirmBlurred] = React.useState(false);
  const [showPassword, setShowPassword] = React.useState(false);
  const [showConfirm, setShowConfirm] = React.useState(false);

  const mismatch = passwordMismatchError(password, confirm, confirmBlurred);
  const weakHint = weakSecretHint(password);

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    onSubmit(password);
  };

  return (
    <form onSubmit={submit} className="flex flex-col gap-(--space-4)">
      {email === undefined ? null : <Input label="Work email" value={email} disabled placeholder="name@company.com" />}
      <Input
        label="New password"
        type={showPassword ? "text" : "password"}
        autoComplete="new-password"
        required
        autoFocus
        value={password}
        onChange={(event) => setPassword(event.target.value)}
        trailing={
          <Button
            variant="ghost"
            size="small"
            onClick={() => setShowPassword((current) => !current)}
            aria-label={showPassword ? "Hide new password" : "Show new password"}
          >
            {showPassword ? "Hide" : "Show"}
          </Button>
        }
      />
      {weakHint ? <p className="text-body text-(--text-muted)">{weakHint}</p> : null}
      <Input
        label="Confirm password"
        type={showConfirm ? "text" : "password"}
        autoComplete="new-password"
        required
        value={confirm}
        onChange={(event) => setConfirm(event.target.value)}
        onBlur={() => setConfirmBlurred(true)}
        {...(mismatch === null ? {} : { error: mismatch })}
        trailing={
          <Button
            variant="ghost"
            size="small"
            onClick={() => setShowConfirm((current) => !current)}
            aria-label={showConfirm ? "Hide confirmation" : "Show confirmation"}
          >
            {showConfirm ? "Hide" : "Show"}
          </Button>
        }
      />
      <Button type="submit" variant="primary" loading={pending} disabled={!canSubmitNewPassword(password, confirm)}>
        {submitLabel}
      </Button>
    </form>
  );
}
