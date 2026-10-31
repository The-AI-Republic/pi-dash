// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Copy-to-clipboard URL field (ADM-033, from parity/specs/admin.md
// §4.11): the labeled read-only URL with a copy affordance. Clicking
// writes the shown URL to the clipboard and raises the informational
// notice; the optional help link round-trips the live credential
// draft so provider consoles stay one click away.
import { Copy } from "lucide-react";
import * as React from "react";

import { notifySuccess } from "../../../shared/shell/index.js";
import {
  PROVIDER_COPY_DONE_TITLE,
  providerCopyDoneMessage,
  resolveCopyHelpHref,
  type CopyHelpHref,
} from "../providerAuth.js";

export interface AdminAuthCopyFieldProps {
  /** Field label, pinned by the parity scenario ("Origin URL", "Callback URI", "Callback URL"). */
  label: string;
  /** Shown URL; also the clipboard payload. */
  url: string;
  /** Help copy under the field (provider-console guidance arrives in NEWFRONT-511). */
  description?: React.ReactNode | undefined;
  /** Optional help link target, static or derived from the live draft. */
  helpHref?: CopyHelpHref | undefined;
  /** Help link text; the link renders only with both text and target. */
  helpLabel?: string | undefined;
  /** Live credential draft for derived help targets. */
  fieldValues?: Record<string, string> | undefined;
}

/** Stable field id from the label for the gate driver ("Callback URI" -> "callback-uri"). */
function labelSlug(label: string): string {
  return label.toLowerCase().replace(/[^a-z0-9]+/g, "-");
}

export function AdminAuthCopyField({
  label,
  url,
  description,
  helpHref,
  helpLabel,
  fieldValues,
}: AdminAuthCopyFieldProps): React.ReactElement {
  const helpTarget = resolveCopyHelpHref(helpHref, fieldValues ?? {});
  const copy = (): void => {
    void navigator.clipboard.writeText(url);
    notifySuccess(PROVIDER_COPY_DONE_TITLE, providerCopyDoneMessage(label));
  };
  return (
    <div className="flex flex-col gap-(--space-1)">
      <p className="text-meta font-medium text-(--text-muted)">{label}</p>
      <button
        type="button"
        onClick={copy}
        data-testid={`admin-auth-copy-${labelSlug(label)}`}
        className="flex w-full cursor-pointer items-center justify-between gap-(--space-3) rounded-(--radius-control) border border-(--border) bg-(--subtle) px-(--space-4) py-(--space-3) text-left"
      >
        <span className="text-body min-w-0 flex-1 truncate font-medium text-(--text)">{url}</span>
        <Copy size={16} aria-hidden="true" className="shrink-0 text-(--text-muted)" />
      </button>
      {description === undefined ? null : <div className="text-meta text-(--text-muted)">{description}</div>}
      {helpTarget !== null && helpLabel !== undefined ? (
        <a href={helpTarget} className="text-meta font-medium text-(--accent) hover:underline">
          {helpLabel}
        </a>
      ) : null}
    </div>
  );
}
