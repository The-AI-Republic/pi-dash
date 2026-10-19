// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Token creation and one-time reveal (parity/specs/workspace-settings.md
// §§26-27, WSET-026/027). The form validates the title inline, requires an
// expiry unless never-expires is on, and previews the resulting expiry.
// Success swaps the same dialog to the one-time secret with copy and a CSV
// backup; closing that step discards the secret from memory.

import type { CreatedPersonalApiToken } from "@pidash/api-client";
import { Button, Dialog, IconButton, Input, Textarea } from "@pidash/kit";
import { Check, Copy } from "lucide-react";
import * as React from "react";

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { useCreatePersonalToken } from "./api.js";
import { downloadTokenBackup } from "./download.js";
import {
  backupFileName,
  emptyTokenDraft,
  EXPIRY_REQUIRED_NOTICE,
  revealExpiryLine,
  resolveExpiry,
  titleError,
  TOKEN_COPIED_NOTICE,
  TOKEN_BACKUP_FAILED_NOTICE,
  TOKEN_CREATE_DESCRIPTION,
  TOKEN_CREATE_TITLE,
  TOKEN_REVEAL_TITLE,
  tokenBackupCsv,
  type TokenDraft,
} from "./model.js";
import { TokenExpiryControls } from "./TokenExpiryPicker.js";

/** Where the dialog is: composing a token, or showing its one-time secret. */
type DialogPhase = { kind: "form" } | { kind: "reveal"; created: CreatedPersonalApiToken };

export interface CreateTokenDialogProps {
  open: boolean;
  onClose: () => void;
  now?: Date | undefined;
}

async function writeClipboard(text: string): Promise<void> {
  if (typeof navigator === "undefined" || navigator.clipboard === undefined) {
    throw new Error("Clipboard is unavailable");
  }
  await navigator.clipboard.writeText(text);
}

export function CreateTokenDialog({ open, onClose, now }: CreateTokenDialogProps): React.ReactElement {
  const create = useCreatePersonalToken();
  const [draft, setDraft] = React.useState<TokenDraft>(emptyTokenDraft);
  const [submitted, setSubmitted] = React.useState(false);
  const [phase, setPhase] = React.useState<DialogPhase>({ kind: "form" });

  const reset = (): void => {
    setDraft(emptyTokenDraft());
    setSubmitted(false);
    setPhase({ kind: "form" });
  };

  const close = (): void => {
    reset();
    onClose();
  };

  const handleOpenChange = (next: boolean): void => {
    if (next) return;
    if (create.isPending) return;
    if (phase.kind === "reveal") {
      reset();
      onClose();
      return;
    }
    close();
  };

  const fieldError = submitted ? titleError(draft.title) : null;

  function submit(): void {
    setSubmitted(true);
    if (titleError(draft.title) !== null) return;
    const current = now ?? new Date();
    const expiredAt = resolveExpiry(draft, current);
    if (expiredAt === undefined) {
      notifyError(EXPIRY_REQUIRED_NOTICE);
      return;
    }
    create.mutate(
      { label: draft.title.trim(), description: draft.description, expired_at: expiredAt },
      {
        onSuccess: (created) => {
          try {
            downloadTokenBackup(
              backupFileName(current),
              tokenBackupCsv({
                title: created.label,
                description: created.description,
                expiry: revealExpiryLine(created.expired_at),
                secret: created.token,
              })
            );
          } catch {
            notifyError(TOKEN_BACKUP_FAILED_NOTICE);
          }
          setPhase({ kind: "reveal", created });
        },
      }
    );
  }

  if (phase.kind === "reveal") {
    return <RevealTokenDialog created={phase.created} onClose={close} />;
  }

  return (
    <Dialog
      title={TOKEN_CREATE_TITLE}
      description={TOKEN_CREATE_DESCRIPTION}
      open={open}
      onOpenChange={handleOpenChange}
      ignoreEscapeKey={false}
      nonDismissable={create.isPending}
      closeButton={false}
      actions={
        <>
          <Button variant="secondary" onClick={close} disabled={create.isPending}>
            Cancel
          </Button>
          <Button variant="primary" loading={create.isPending} onClick={submit}>
            Generate token
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-(--space-6)">
        <Input
          label="Title"
          placeholder="Title"
          value={draft.title}
          {...(fieldError === null ? {} : { error: fieldError })}
          onChange={(event) => setDraft({ ...draft, title: event.target.value })}
        />
        <Textarea
          label="Description"
          placeholder="Description"
          rows={3}
          value={draft.description}
          onChange={(event) => setDraft({ ...draft, description: event.target.value })}
        />
        <TokenExpiryControls draft={draft} onChange={setDraft} {...(now === undefined ? {} : { now })} />
      </div>
    </Dialog>
  );
}

interface RevealTokenDialogProps {
  created: CreatedPersonalApiToken;
  onClose: () => void;
}

/** The one-time secret step: copy first, then explicit Close. */
function RevealTokenDialog({ created, onClose }: RevealTokenDialogProps): React.ReactElement {
  const [copied, setCopied] = React.useState(false);
  const copy = (): void => {
    writeClipboard(created.token)
      .then(() => {
        setCopied(true);
        notifySuccess(TOKEN_COPIED_NOTICE);
      })
      .catch(() => {
        notifyError("Could not copy the token");
      });
  };
  return (
    <Dialog
      title={TOKEN_REVEAL_TITLE}
      description="Store this secret somewhere safe. It is shown only once and cannot be retrieved later."
      open
      ignoreEscapeKey
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
      closeButton={false}
      actions={
        <Button variant="primary" onClick={onClose}>
          Close
        </Button>
      }
    >
      <div className="flex flex-col gap-(--space-4)">
        <div className="flex items-center gap-(--space-2) rounded-(--radius-control) border border-(--border) bg-(--subtle) px-(--space-3) py-(--space-2)">
          <code data-testid="token-secret" className="text-meta min-w-0 flex-1 break-all text-(--text)">
            {created.token}
          </code>
          <IconButton label={copied ? "Token copied" : "Copy token"} onClick={copy}>
            {copied ? <Check size={16} strokeWidth={1.5} /> : <Copy size={16} strokeWidth={1.5} />}
          </IconButton>
        </div>
        <p className="text-body text-(--text-muted)">{revealExpiryLine(created.expired_at)}</p>
        <p className="text-meta text-(--text-muted)">A backup file has been downloaded for you.</p>
      </div>
    </Dialog>
  );
}
