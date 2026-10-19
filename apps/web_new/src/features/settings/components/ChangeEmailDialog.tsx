// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Two-step email change dialog (parity/specs/profile.md §6, PROF-013).
// Step one checks the address shape and its availability, then asks the
// server to mail a one-time code; step two locks the address, verifies the
// code and, on success, lets the caller sign the member out.

import { Button, Dialog, Input } from "@pidash/kit";
import * as React from "react";

import {
  EMAIL_MESSAGES,
  emailCodeError,
  emailStepOutcome,
  emailShapeError,
  emailVerifyMessage,
  type EmailChangeStep,
} from "../lib/email.js";
import { useCheckEmailTaken, useConfirmEmailChange, useGenerateEmailCode } from "../api.js";

export interface ChangeEmailDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The account's current address; an unchanged address is refused. */
  currentEmail: string;
  /** Runs after a successful move (the caller signs out and navigates). */
  onChanged: () => void;
}

export function ChangeEmailDialog({
  open,
  onOpenChange,
  currentEmail,
  onChanged,
}: ChangeEmailDialogProps): React.ReactElement {
  const [step, setStep] = React.useState<EmailChangeStep>("email");
  const [email, setEmail] = React.useState("");
  const [code, setCode] = React.useState("");
  const [emailError, setEmailError] = React.useState<string | null>(null);
  const [codeError, setCodeError] = React.useState<string | null>(null);
  const checkTaken = useCheckEmailTaken();
  const sendCode = useGenerateEmailCode();
  const confirm = useConfirmEmailChange();

  React.useEffect(() => {
    if (open) return;
    setStep("email");
    setEmail("");
    setCode("");
    setEmailError(null);
    setCodeError(null);
  }, [open]);

  const busy = checkTaken.isPending || sendCode.isPending || confirm.isPending;

  const submit = async (event: React.FormEvent): Promise<void> => {
    event.preventDefault();
    if (busy) return;
    if (step === "email") {
      const shape = emailShapeError(email, currentEmail);
      if (shape !== null) {
        setEmailError(shape);
        return;
      }
      setEmailError(null);
      let existing: boolean;
      try {
        existing = await checkTaken.mutateAsync(email.trim().toLowerCase());
      } catch (error) {
        setEmailError(emailVerifyMessage(error));
        return;
      }
      const outcome = existing
        ? emailStepOutcome({ existing })
        : await sendCode.mutateAsync(email.trim().toLowerCase()).then(
            () => emailStepOutcome({ existing: false }),
            (error: unknown) => emailStepOutcome({ existing: false, sendFailure: error })
          );
      if (outcome.kind === "sent") {
        setStep("code");
        setCodeError(null);
        return;
      }
      setEmailError(outcome.message);
      return;
    }
    const error = emailCodeError(code);
    if (error !== null) {
      setCodeError(error);
      return;
    }
    setCodeError(null);
    try {
      await confirm.mutateAsync({ email: email.trim().toLowerCase(), code: code.trim() });
    } catch (failure) {
      setCodeError(emailVerifyMessage(failure));
      return;
    }
    onChanged();
  };

  return (
    <Dialog
      title="Change email"
      description="Enter a new email address to receive a verification link."
      open={open}
      onOpenChange={(next) => {
        if (busy) return;
        onOpenChange(next);
      }}
      nonDismissable={busy}
      actions={
        <>
          <Button variant="secondary" onClick={() => onOpenChange(false)} disabled={busy}>
            Cancel
          </Button>
          <Button type="submit" form="change-email-form" variant="primary" loading={busy}>
            {step === "email" ? "Continue" : "Confirm"}
          </Button>
        </>
      }
    >
      <form
        id="change-email-form"
        noValidate
        className="flex flex-col gap-(--space-4)"
        onSubmit={(event) => void submit(event)}
      >
        <Input
          label="New email"
          id="change-email-address"
          type="email"
          autoComplete="off"
          placeholder="Enter your email"
          value={email}
          disabled={step === "code"}
          {...({ error: emailError ?? undefined } as { error?: string })}
          onChange={(event) => {
            setEmail(event.target.value);
            setEmailError(null);
          }}
        />
        {step === "code" ? (
          <>
            <Input
              label="Unique code"
              id="change-email-code"
              autoComplete="one-time-code"
              placeholder="123456"
              value={code}
              {...({ error: codeError ?? undefined } as { error?: string })}
              onChange={(event) => {
                setCode(event.target.value);
                setCodeError(null);
              }}
            />
            <p className="text-meta text-(--success)">{EMAIL_MESSAGES.sent}</p>
          </>
        ) : null}
      </form>
    </Dialog>
  );
}
