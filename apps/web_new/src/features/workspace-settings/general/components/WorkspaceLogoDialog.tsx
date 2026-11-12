// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Logo dialog (parity/specs/workspace-settings.md §4, WSET-004). One image,
// picked or dropped, limited to JPEG/PNG/WebP at 5 MB with inline refusals.
// Remove sits on the left ("Removing" while it works) and stays disabled
// without a stored logo; Cancel and "Upload & Save" ("Uploading" while it
// works) sit on the right, the primary disabled until a file is picked.
// Upload and removal both close the dialog on success; a failure keeps it
// open and toasts through the mutation. Escape and the backdrop dismiss.

import { Button, Dialog } from "@pidash/kit";
import { ImagePlus } from "lucide-react";
import * as React from "react";

import { useRemoveWorkspaceLogo, useUploadWorkspaceLogo } from "../api/mutations.js";
import {
  isLegacyLogoValue,
  LOGO_ACCEPT_ATTRIBUTE,
  LOGO_ACCEPTED_TYPES,
  LOGO_MAX_BYTES,
  validateLogoFile,
} from "../rules.js";

export const LOGO_DIALOG_TITLE = "Upload image";
export const LOGO_DIALOG_HINT = "JPEG, PNG or WebP, up to 5 MB.";

export interface WorkspaceLogoDialogProps {
  workspaceSlug: string;
  workspaceId: string;
  /** The stored logo URL, or null when the initial fallback shows. */
  logoUrl: string | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function WorkspaceLogoDialog({
  workspaceSlug,
  workspaceId,
  logoUrl,
  open,
  onOpenChange,
}: WorkspaceLogoDialogProps): React.ReactElement {
  const upload = useUploadWorkspaceLogo(workspaceSlug);
  const remove = useRemoveWorkspaceLogo(workspaceSlug);
  const [file, setFile] = React.useState<File | null>(null);
  const [fileError, setFileError] = React.useState<string | null>(null);
  const [dragging, setDragging] = React.useState(false);
  // Browsers preview the picked file locally; environments without an
  // object-URL API (jsdom) simply keep showing the stored image.
  const preview = React.useMemo(() => {
    if (file === null) return null;
    if (typeof URL.createObjectURL !== "function") return null;
    return URL.createObjectURL(file);
  }, [file]);
  const uploading = upload.isPending;
  const removing = remove.isPending;
  // One write at a time: two clicks in the same frame both read the last
  // render's pending flags, so a ref closes the gap.
  const inFlight = React.useRef(false);

  React.useEffect(
    () => () => {
      if (preview !== null && typeof URL.revokeObjectURL === "function") URL.revokeObjectURL(preview);
    },
    [preview]
  );

  const pick = (next: File | null): void => {
    if (next === null) {
      setFile(null);
      setFileError(null);
      return;
    }
    const problem = validateLogoFile(next);
    setFileError(problem);
    setFile(problem === null ? next : null);
  };

  const close = (): void => {
    setFile(null);
    setFileError(null);
    onOpenChange(false);
  };

  const submit = (): void => {
    if (file === null || inFlight.current) return;
    inFlight.current = true;
    upload.mutate(
      { workspaceId, file },
      {
        onSuccess: () => {
          close();
        },
        onSettled: () => {
          inFlight.current = false;
        },
      }
    );
  };

  const removeLogo = (): void => {
    if (logoUrl === null || inFlight.current) return;
    inFlight.current = true;
    remove.mutate(
      { workspaceId, logoUrl, legacy: isLegacyLogoValue(logoUrl) },
      {
        onSuccess: () => {
          close();
        },
        onSettled: () => {
          inFlight.current = false;
        },
      }
    );
  };

  const showsImage = preview ?? logoUrl;

  return (
    <Dialog
      open={open}
      // Controlled: the dialog only reports a dismissal, never a re-open.
      onOpenChange={() => {
        close();
      }}
      title={LOGO_DIALOG_TITLE}
      actions={
        <div className="flex w-full items-center justify-between gap-(--space-4)">
          <Button
            variant="danger"
            loading={removing}
            disabled={logoUrl === null || uploading || removing}
            onClick={removeLogo}
          >
            {removing ? "Removing" : "Remove"}
          </Button>
          <div className="flex items-center gap-(--space-2)">
            <Button variant="secondary" onClick={close}>
              Cancel
            </Button>
            <Button
              variant="primary"
              loading={uploading}
              disabled={file === null || uploading || removing}
              onClick={submit}
            >
              {uploading ? "Uploading" : "Upload & Save"}
            </Button>
          </div>
        </div>
      }
    >
      <div className="flex flex-col gap-(--space-3)">
        <label
          onDragOver={(event) => {
            event.preventDefault();
            setDragging(true);
          }}
          onDragLeave={() => {
            setDragging(false);
          }}
          onDrop={(event) => {
            event.preventDefault();
            setDragging(false);
            pick(event.dataTransfer.files[0] ?? null);
          }}
          className={`grid aspect-square w-full cursor-pointer place-items-center overflow-hidden rounded-(--radius-control) border-2 border-dashed p-(--space-6) text-center ${
            dragging ? "border-(--accent) bg-(--subtle)" : "border-(--border)"
          }`}
        >
          <input
            type="file"
            accept={LOGO_ACCEPT_ATTRIBUTE}
            data-max-bytes={LOGO_MAX_BYTES}
            data-accepted-types={LOGO_ACCEPTED_TYPES.join(" ")}
            className="sr-only"
            onChange={(event) => {
              pick(event.target.files?.[0] ?? null);
            }}
          />
          {showsImage === null || showsImage === undefined ? (
            <span className="flex flex-col items-center gap-(--space-2) text-(--text-muted)">
              <ImagePlus size={32} strokeWidth={1.25} aria-hidden="true" />
              <span className="text-body">{dragging ? "Drop the image here" : "Choose or drop an image"}</span>
            </span>
          ) : (
            <img src={showsImage} alt="" className="size-full object-contain" />
          )}
        </label>
        {fileError === null ? (
          <p className="text-meta text-(--text-muted)">{LOGO_DIALOG_HINT}</p>
        ) : (
          <p role="alert" className="text-meta text-(--danger)">
            {fileError}
          </p>
        )}
      </div>
    </Dialog>
  );
}
