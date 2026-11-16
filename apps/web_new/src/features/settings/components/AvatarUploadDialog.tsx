// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Avatar upload dialog (parity/specs/profile.md §6, PROF-012). One picker
// rejects non-images and files above five megabytes inline; a valid pick
// uploads and auto-saves, while the remove action clears the photo back to
// the placeholder and persists the removal.

import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

import { AVATAR_ACCEPT, avatarFileError } from "../lib/avatar.js";

export interface AvatarUploadDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The stored photo, when there is one. */
  currentSrc: string | null;
  uploading: boolean;
  removing: boolean;
  onUpload: (file: File) => void;
  onRemove: () => void;
}

export function AvatarUploadDialog({
  open,
  onOpenChange,
  currentSrc,
  uploading,
  removing,
  onUpload,
  onRemove,
}: AvatarUploadDialogProps): React.ReactElement {
  const [file, setFile] = React.useState<File | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const inputRef = React.useRef<HTMLInputElement>(null);

  React.useEffect(() => {
    if (open) return;
    // Reopening starts clean; the dialog owns no state between visits.
    setFile(null);
    setError(null);
  }, [open]);

  const pick = (picked: File | null): void => {
    if (picked === null) {
      setFile(null);
      return;
    }
    const rejection = avatarFileError(picked);
    if (rejection !== null) {
      setFile(null);
      setError(rejection);
      return;
    }
    setError(null);
    setFile(picked);
  };

  const busy = uploading || removing;
  const hasPhoto = currentSrc !== null && currentSrc !== "";

  return (
    <Dialog
      title="Upload Image"
      open={open}
      onOpenChange={(next) => {
        if (busy) return;
        onOpenChange(next);
      }}
      nonDismissable={busy}
      className="max-w-xl"
      actions={
        <>
          <Button variant="danger" onClick={onRemove} disabled={!hasPhoto || busy} loading={removing}>
            Remove
          </Button>
          <div className="ml-auto flex items-center gap-(--space-2)">
            <Button variant="secondary" onClick={() => onOpenChange(false)} disabled={busy}>
              Cancel
            </Button>
            <Button
              variant="primary"
              onClick={() => {
                if (file !== null) onUpload(file);
              }}
              disabled={file === null || busy}
              loading={uploading}
            >
              Upload &amp; Save
            </Button>
          </div>
        </>
      }
    >
      <div className="flex flex-col gap-(--space-3)">
        <button
          type="button"
          onClick={() => inputRef.current?.click()}
          className="grid h-64 w-full cursor-pointer place-items-center overflow-hidden rounded-(--radius-control) border-2 border-dashed border-(--border) bg-(--subtle)"
        >
          {file !== null || hasPhoto ? (
            <img
              src={file !== null ? URL.createObjectURL(file) : (currentSrc ?? "")}
              alt="Selected profile photo"
              className="size-full object-cover"
            />
          ) : (
            <span className="text-body text-(--text-muted)">Drag &amp; drop an image or click to choose one</span>
          )}
        </button>
        <input
          ref={inputRef}
          type="file"
          accept={AVATAR_ACCEPT}
          className="sr-only"
          data-testid="avatar-file-input"
          onChange={(event) => {
            pick(event.target.files?.[0] ?? null);
            // Allow picking the same file again after a rejection.
            event.target.value = "";
          }}
        />
        {error !== null ? (
          <p role="alert" className="text-meta text-(--danger)">
            {error}
          </p>
        ) : (
          <p className="text-meta text-(--text-muted)">JPEG, PNG, WebP or GIF, up to 5 MB.</p>
        )}
      </div>
    </Dialog>
  );
}
