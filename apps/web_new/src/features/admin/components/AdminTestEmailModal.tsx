// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Send-test-email modal (ADM-021, from parity/specs/admin.md §3.4): a
// three-step flow — compose a receiver address, then a success step
// (advising to check spam and re-verify settings) or a failure step
// showing the server's error message. The parent unmounts the modal on
// close, so every opening composes fresh. The check POST carries the
// receiver together with the SMTP values the form shows, so the server
// grades the entered settings even when they are not saved yet
// (NEWFRONT-477).
import { Button, Dialog, Input } from "@pidash/kit";
import * as React from "react";

import type { EmailFormValues } from "../emailConfig.js";
import { useCheckEmailCredentials } from "../hooks.js";

/** Compose-step heading (wording pinned by the parity scenario). */
export const TEST_EMAIL_COMPOSE_TITLE = "Send test email";
/** Success-step heading (wording pinned by the parity scenario). */
export const TEST_EMAIL_SUCCESS_TITLE = "Email send";
/** Failure-step heading (wording pinned by the parity scenario). */
export const TEST_EMAIL_FAILED_TITLE = "Failed";

export type TestEmailStep = "compose" | "success" | "failed";

export interface AdminTestEmailModalProps {
  /** The form values to grade — entered, possibly unsaved. */
  candidate: EmailFormValues;
  onClose: () => void;
}

export function AdminTestEmailModal({ candidate, onClose }: AdminTestEmailModalProps): React.ReactElement {
  const [receiver, setReceiver] = React.useState("");
  const [step, setStep] = React.useState<TestEmailStep>("compose");
  const [failure, setFailure] = React.useState("");
  const check = useCheckEmailCredentials();

  const title =
    step === "success"
      ? TEST_EMAIL_SUCCESS_TITLE
      : step === "failed"
        ? TEST_EMAIL_FAILED_TITLE
        : TEST_EMAIL_COMPOSE_TITLE;

  const send = () => {
    check.mutate(
      { receiverEmail: receiver, candidate },
      {
        onSuccess: () => setStep("success"),
        // The client rejects with ApiError, whose message is the server's
        // own error text, shown verbatim on the failure step.
        onError: (error) => {
          setFailure(error.message);
          setStep("failed");
        },
      }
    );
  };

  return (
    <Dialog
      open
      onOpenChange={() => onClose()}
      title={title}
      actions={
        step === "compose" ? (
          <>
            <Button variant="secondary" onClick={onClose} data-testid="admin-test-email-dismiss">
              Cancel
            </Button>
            <Button variant="primary" onClick={send} loading={check.isPending} data-testid="admin-test-email-send">
              Send email
            </Button>
          </>
        ) : (
          <Button variant="secondary" onClick={onClose} data-testid="admin-test-email-dismiss">
            Close
          </Button>
        )
      }
    >
      <div data-testid="admin-test-email-step" data-step={step}>
        {step === "compose" ? (
          <Input
            label="Recipient email"
            id="admin-test-email-receiver"
            type="email"
            autoComplete="email"
            value={receiver}
            onChange={(event) => setReceiver(event.target.value)}
            data-testid="admin-test-email-receiver"
          />
        ) : step === "success" ? (
          <p data-testid="admin-test-email-body" className="text-body text-(--text)">
            A test message is on its way to {receiver}. If it does not arrive, check the spam folder and confirm the
            mail settings.
          </p>
        ) : (
          <p data-testid="admin-test-email-body" role="alert" className="text-body text-(--text)">
            {failure}
          </p>
        )}
      </div>
    </Dialog>
  );
}
