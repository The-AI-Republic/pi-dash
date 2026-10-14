// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Forgot-password card. Posts the address to the reset-link endpoint,
// toasts the inbox-check confirmation on success, and throttles resends
// behind a thirty-second countdown that disables the email field.
// Failures toast without leaking account existence. Written from
// parity/specs/auth.md AUTH-012 on the sign-in card's conventions.

import { Button, IconButton, Input } from "@pidash/kit";
import { X } from "lucide-react";
import * as React from "react";

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { useForgotPassword } from "../api.js";
import {
  describeForgotPasswordFailure,
  FORGOT_FAILURE_TITLE,
  FORGOT_RESEND_COOLDOWN_S,
  FORGOT_SUCCESS_TITLE,
  forgotSubmitLabel,
  forgotSuccessDescription,
  resendCountdown,
} from "../recovery.js";
import { isValidEmail } from "./SignInCard.js";

export interface ForgotPasswordCardProps {
  initialEmail?: string | undefined;
}

export function ForgotPasswordCard({ initialEmail = "" }: ForgotPasswordCardProps): React.ReactElement {
  const [email, setEmail] = React.useState(initialEmail.trim());
  const [emailTouched, setEmailTouched] = React.useState(false);
  const [lastSentAt, setLastSentAt] = React.useState<number | null>(null);
  const [now, setNow] = React.useState(() => Date.now());
  const forgot = useForgotPassword();

  const resendIn = resendCountdown(lastSentAt, now, FORGOT_RESEND_COOLDOWN_S);
  const coolingDown = resendIn > 0;

  React.useEffect(() => {
    if (!coolingDown) return;
    const timer = globalThis.setInterval(() => setNow(Date.now()), 1000);
    return () => globalThis.clearInterval(timer);
  }, [coolingDown]);
  const sent = lastSentAt !== null;
  const showInvalid = emailTouched && email.length > 0 && !isValidEmail(email);
  const submitDisabled = !isValidEmail(email) || forgot.isPending || coolingDown;

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    const address = email.trim();
    forgot.mutate(address, {
      onSuccess: () => {
        notifySuccess(FORGOT_SUCCESS_TITLE, forgotSuccessDescription(address));
        setLastSentAt(Date.now());
        setNow(Date.now());
      },
      onError: (error) => {
        notifyError(FORGOT_FAILURE_TITLE, describeForgotPasswordFailure(error));
      },
    });
  };

  return (
    <section aria-label="Forgot password" className="flex w-full max-w-90 flex-col gap-(--space-6)">
      <div className="flex flex-col gap-(--space-1)">
        <h1 className="text-h2 text-(--text)">Reset your password</h1>
        <p className="text-body text-(--text-muted)">Enter your work address and we will send a reset link.</p>
      </div>
      <form onSubmit={submit} className="flex flex-col gap-(--space-4)">
        <Input
          label="Work email"
          type="email"
          autoComplete="email"
          required
          autoFocus
          placeholder="name@company.com"
          value={email}
          disabled={coolingDown}
          onChange={(event) => setEmail(event.target.value)}
          onBlur={() => setEmailTouched(true)}
          {...(showInvalid ? { error: "Enter a valid email address." } : {})}
          {...(email.length > 0 && !coolingDown
            ? {
                trailing: (
                  <IconButton
                    label="Clear"
                    size="small"
                    onMouseDown={(event) => event.preventDefault()}
                    onClick={() => setEmail("")}
                  >
                    <X size={14} aria-hidden="true" />
                  </IconButton>
                ),
              }
            : {})}
        />
        <Button type="submit" variant="primary" loading={forgot.isPending} disabled={submitDisabled}>
          {forgotSubmitLabel(resendIn, sent)}
        </Button>
      </form>
      <a href="/" className="text-body text-(--accent) hover:underline">
        Back to sign in
      </a>
    </section>
  );
}
