// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Security tab (parity/specs/profile.md §6, PROF-015). Accounts created
// through a provider skip the current-password field; the new secret must
// pass the strength meter, differ from the current one and match its
// confirmation. Each field reveals independently, the mismatch note waits
// for a blur or a long-enough guess, and server failures map to a field
// error or a toast.

import { Button, Input } from "@pidash/kit";
import { Eye, EyeOff } from "lucide-react";
import * as React from "react";

import { useMe } from "../../../core/session/index.js";
import { useChangePassword } from "../api.js";
import {
  canSubmitPassword,
  classifyPasswordFieldError,
  describePasswordFailure,
  mismatchVisible,
  passwordRules,
  sameAsOldVisible,
  strengthVisible,
  type PasswordField,
  type PasswordFormState,
} from "../lib/password.js";

interface RevealButtonProps {
  field: PasswordField;
  visible: boolean;
  onToggle: () => void;
}

function RevealButton({ field, visible, onToggle }: RevealButtonProps): React.ReactElement {
  return (
    <button
      type="button"
      aria-label={visible ? `Hide ${field} password` : `Show ${field} password`}
      aria-pressed={visible}
      data-testid={`password-reveal-${field}`}
      className="inline-flex size-6 items-center justify-center rounded-(--radius-control) text-(--text-muted) hover:text-(--text)"
      onClick={onToggle}
    >
      {visible ? (
        <EyeOff size={16} strokeWidth={1.5} aria-hidden="true" />
      ) : (
        <Eye size={16} strokeWidth={1.5} aria-hidden="true" />
      )}
    </button>
  );
}

export function SecuritySettingsTab(): React.ReactElement {
  const { data: me } = useMe();
  const changePassword = useChangePassword();
  const [values, setValues] = React.useState({ current: "", next: "", confirm: "" });
  const [focused, setFocused] = React.useState<PasswordField | null>(null);
  const [confirmTouched, setConfirmTouched] = React.useState(false);
  const [revealed, setRevealed] = React.useState<Record<PasswordField, boolean>>({
    current: false,
    next: false,
    confirm: false,
  });
  const [serverErrors, setServerErrors] = React.useState<Partial<Record<"current" | "new", string>>>({});

  if (me === undefined) {
    return (
      <div data-testid="security-settings-loading" aria-busy="true" className="flex flex-col gap-(--space-3)">
        <div className="h-10 w-full animate-pulse rounded-(--radius-control) bg-(--subtle)" />
        <div className="h-10 w-full animate-pulse rounded-(--radius-control) bg-(--subtle)" />
      </div>
    );
  }

  const requiresCurrent = me.is_password_autoset !== true;
  const state: PasswordFormState = { requiresCurrent, focused, confirmTouched, values };
  const mismatch = mismatchVisible(state);
  const sameAsOld = sameAsOldVisible(state);
  const showStrength = strengthVisible(values.next);
  const canSubmit = canSubmitPassword(state) && !changePassword.isPending;

  const setValue = (field: PasswordField, value: string): void => {
    setValues((previous) => ({ ...previous, [field]: value }));
    if (field === "current" || field === "next") {
      setServerErrors((previous) => {
        const next = { ...previous };
        delete next[field === "current" ? "current" : "new"];
        return next;
      });
    }
  };

  const submit = (event: React.FormEvent): void => {
    event.preventDefault();
    if (!canSubmitPassword(state)) return;
    changePassword.mutate(
      {
        ...(requiresCurrent ? { oldPassword: values.current } : {}),
        newPassword: values.next,
      },
      {
        onSuccess: () => {
          setValues({ current: "", next: "", confirm: "" });
          setFocused(null);
          setConfirmTouched(false);
          setServerErrors({});
          setRevealed({ current: false, next: false, confirm: false });
        },
        onError: (error) => {
          const field = classifyPasswordFieldError(error);
          setServerErrors(field === null ? {} : { [field]: describePasswordFailure(error).message });
        },
      }
    );
  };

  return (
    <div className="flex flex-col gap-(--space-6)">
      <h1 className="text-h3 font-semibold text-(--text)">Change password</h1>
      <form data-testid="security-settings-form" noValidate className="flex flex-col gap-(--space-6)" onSubmit={submit}>
        {requiresCurrent ? (
          <Input
            label="Current password"
            id="old_password"
            name="old_password"
            type={revealed.current ? "text" : "password"}
            autoComplete="current-password"
            value={values.current}
            {...({ error: serverErrors.current } as { error?: string })}
            onChange={(event) => setValue("current", event.target.value)}
            onFocus={() => setFocused("current")}
            onBlur={() => setFocused(null)}
            trailing={
              <RevealButton
                field="current"
                visible={revealed.current}
                onToggle={() => setRevealed((previous) => ({ ...previous, current: !previous.current }))}
              />
            }
          />
        ) : null}

        <div className="grid gap-(--space-4) sm:grid-cols-2">
          <Input
            label="New password"
            id="new_password"
            name="new_password"
            type={revealed.next ? "text" : "password"}
            autoComplete="new-password"
            placeholder="Enter new password"
            value={values.next}
            {...({ error: serverErrors.new } as { error?: string })}
            onChange={(event) => setValue("next", event.target.value)}
            onFocus={() => setFocused("next")}
            onBlur={() => setFocused(null)}
            trailing={
              <RevealButton
                field="next"
                visible={revealed.next}
                onToggle={() => setRevealed((previous) => ({ ...previous, next: !previous.next }))}
              />
            }
          />
          <Input
            label="Confirm password"
            id="confirm_password"
            name="confirm_password"
            type={revealed.confirm ? "text" : "password"}
            autoComplete="new-password"
            placeholder="Confirm password"
            value={values.confirm}
            onChange={(event) => setValue("confirm", event.target.value)}
            onFocus={() => setFocused("confirm")}
            onBlur={() => {
              setFocused(null);
              setConfirmTouched(true);
            }}
            trailing={
              <RevealButton
                field="confirm"
                visible={revealed.confirm}
                onToggle={() => setRevealed((previous) => ({ ...previous, confirm: !previous.confirm }))}
              />
            }
          />
        </div>

        {showStrength ? (
          <ul data-testid="password-strength" className="flex flex-wrap gap-(--space-3)">
            {passwordRules(values.next).map((rule) => (
              <li key={rule.id} className={`text-meta ${rule.met ? "text-(--success)" : "text-(--text-muted)"}`}>
                {rule.label}
              </li>
            ))}
          </ul>
        ) : null}
        {mismatch ? <p className="text-meta text-(--danger)">Passwords don&apos;t match</p> : null}
        {sameAsOld ? (
          <p className="text-meta text-(--danger)">New password must be different from old password</p>
        ) : null}

        <div>
          <Button type="submit" variant="primary" loading={changePassword.isPending} disabled={!canSubmit}>
            {changePassword.isPending ? "Changing password" : "Change password"}
          </Button>
        </div>
      </form>
    </div>
  );
}
