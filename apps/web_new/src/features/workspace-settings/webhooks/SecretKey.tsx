// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One-time secret key (parity/specs/workspace-settings.md §20/§22,
// WSET-021/023). The key arrives from the create or regenerate answer and
// lives only in the caller's memory. It renders masked until asked; the
// show control toggles the in-clear value, the copy control writes it to
// the clipboard. The masked placeholder is a row of dots rather than the
// key with a CSS filter, so the DOM never carries the value while hidden.

import { IconButton } from "@pidash/kit";
import { Copy, Eye, EyeOff } from "lucide-react";
import * as React from "react";

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";

export const SECRET_COPIED_TITLE = "Secret key copied";
export const SECRET_COPY_FAILED_TITLE = "Couldn't copy the secret key";
export const SECRET_COPY_FAILED_BODY = "The browser refused the clipboard write.";
export const SECRET_MASK = "••••••••••••••••••••••••";
export const SECRET_SHOW_LABEL = "Show secret key";
export const SECRET_HIDE_LABEL = "Hide secret key";
export const SECRET_COPY_LABEL = "Copy secret key";

export interface CopySecretDeps {
  writeText: (value: string) => Promise<void>;
  notifySuccess: (title: string) => void;
  notifyError: (title: string, body: string) => void;
}

function defaultCopyDeps(): CopySecretDeps {
  return {
    writeText: (value) => navigator.clipboard.writeText(value),
    notifySuccess: (title) => {
      notifySuccess(title);
    },
    notifyError: (title, body) => {
      notifyError(title, body);
    },
  };
}

export interface SecretKeyProps {
  secret: string;
  /** Called after a successful clipboard write. */
  onCopied?: (() => void) | undefined;
  /** Test seam for the clipboard and notices. */
  copyDeps?: CopySecretDeps | undefined;
}

export function SecretKey({ secret, onCopied, copyDeps }: SecretKeyProps): React.ReactElement {
  const [shown, setShown] = React.useState(false);
  const copy = (): void => {
    const deps = copyDeps ?? defaultCopyDeps();
    void deps
      .writeText(secret)
      .then(() => {
        deps.notifySuccess(SECRET_COPIED_TITLE);
        onCopied?.();
      })
      .catch(() => {
        deps.notifyError(SECRET_COPY_FAILED_TITLE, SECRET_COPY_FAILED_BODY);
      });
  };
  return (
    <div className="flex items-center justify-between gap-(--space-4) rounded-(--radius-control) border border-(--border) bg-(--subtle) px-(--space-4) py-(--space-3)">
      <div className="flex min-w-0 items-center gap-(--space-4)" data-secret-key-box>
        <span className="text-meta shrink-0 text-(--text-muted)">Secret key</span>
        {shown ? (
          <p data-secret-value className="font-mono break-all text-(--text)">
            {secret}
          </p>
        ) : (
          <p data-secret-mask aria-label="Secret key hidden" className="font-mono text-(--text-muted) select-none">
            {SECRET_MASK}
          </p>
        )}
      </div>
      <div className="flex shrink-0 items-center gap-(--space-1)">
        <IconButton label={shown ? SECRET_HIDE_LABEL : SECRET_SHOW_LABEL} onClick={() => setShown((value) => !value)}>
          {shown ? (
            <EyeOff size={16} strokeWidth={1.5} aria-hidden="true" />
          ) : (
            <Eye size={16} strokeWidth={1.5} aria-hidden="true" />
          )}
        </IconButton>
        <IconButton label={SECRET_COPY_LABEL} onClick={copy}>
          <Copy size={16} strokeWidth={1.5} aria-hidden="true" />
        </IconButton>
      </div>
    </div>
  );
}
