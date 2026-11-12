// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Emblem upload (parity/specs/project-settings-general.md §PSET-007). The
// only artwork the tab accepts beyond the emoji/icon mark: an image goes
// through the user-asset grant → bytes → confirm pipeline, and its URL
// becomes the draft mark. A refused upload keeps the dialog open with the
// reason; nothing is persisted until the form saves.

import { Button, Dialog } from "@pidash/kit";
import { Upload } from "lucide-react";
import * as React from "react";

import { emblemFileError } from "../rules.js";

export interface EmblemUploadDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Uploads and resolves the confirmed asset URL. */
  onUpload: (file: File) => Promise<string>;
  /** Receives the uploaded URL; the form shows it as the draft mark. */
  onUploaded: (url: string) => void;
}

export function EmblemUploadDialog({
  open,
  onOpenChange,
  onUpload,
  onUploaded,
}: EmblemUploadDialogProps): React.ReactElement {
  const [error, setError] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState(false);
  const inputRef = React.useRef<HTMLInputElement | null>(null);

  const choose = async (file: File | undefined): Promise<void> => {
    if (file === undefined) return;
    const refusal = emblemFileError(file);
    setError(refusal);
    if (refusal !== null) return;
    setBusy(true);
    try {
      const url = await onUpload(file);
      onUploaded(url);
      onOpenChange(false);
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : "The emblem could not be uploaded.");
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      title="Upload emblem"
      open={open}
      onOpenChange={(next) => {
        if (!next) setError(null);
        onOpenChange(next);
      }}
      data-testid="general-emblem-upload"
    >
      <div className="flex flex-col gap-(--space-4)">
        <p className="text-body text-(--text-muted)">Upload a square image, up to 5 MB.</p>
        <input
          ref={inputRef}
          type="file"
          accept="image/png,image/jpeg,image/webp,image/gif"
          aria-label="Emblem image"
          data-testid="general-emblem-file"
          className="hidden"
          onChange={(event) => {
            void choose(event.target.files?.[0]);
            event.target.value = "";
          }}
        />
        <Button
          variant="secondary"
          loading={busy}
          data-testid="general-emblem-choose"
          onClick={() => inputRef.current?.click()}
        >
          <Upload size={16} strokeWidth={1.5} aria-hidden="true" />
          Choose image
        </Button>
        {error === null ? null : (
          <p role="alert" className="text-body text-(--danger)">
            {error}
          </p>
        )}
      </div>
    </Dialog>
  );
}
