// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The General tab's identity block (parity/specs/project-settings-general.md
// §PSET-005, §PSET-007): the project emblem control, the live name and short
// code, the visibility caption and the creation stamp. There is no cover
// banner anywhere on the tab (target main#523) and no cover control.

import { Button } from "@pidash/kit";
import { Image as ImageIcon, Smile } from "lucide-react";
import * as React from "react";

import { ProjectMark } from "../../../../shared/pickers/index.js";
import { VISIBILITY_OPTIONS } from "../rules.js";

export interface GeneralIdentityProps {
  /** Live name from the draft. */
  name: string;
  /** Live short code from the draft. */
  identifier: string;
  /** Live mark from the draft. */
  logoProps: Record<string, unknown>;
  /** Stored visibility network. */
  network: number;
  /** ISO timestamp the project was created at. */
  createdAt: string;
  /** False renders the mark without its picker (non-admins). */
  canEdit: boolean;
  /** Base URL relative emblem paths resolve against. */
  apiBaseUrl: string | undefined;
  onOpenMark: () => void;
  onOpenUpload: () => void;
}

/** One line describing who can see the project. */
export function visibilityLabel(network: number): string {
  return VISIBILITY_OPTIONS.find((option) => option.value === network)?.label ?? "Private";
}

/** The creation stamp as the tab states it; invalid input reads as empty. */
export function createdLine(createdAt: string, formatter: Intl.DateTimeFormat): string {
  const parsed = new Date(createdAt);
  if (Number.isNaN(parsed.getTime())) return "";
  return `Created on ${formatter.format(parsed)}`;
}

export function GeneralIdentity({
  name,
  identifier,
  logoProps,
  network,
  createdAt,
  canEdit,
  apiBaseUrl,
  onOpenMark,
  onOpenUpload,
}: GeneralIdentityProps): React.ReactElement {
  const created = createdLine(createdAt, new Intl.DateTimeFormat(undefined, { dateStyle: "medium" }));
  return (
    <section aria-label="Project identity" data-testid="general-banner" className="flex items-center gap-(--space-4)">
      <ProjectMark logoProps={logoProps} identifier={identifier} size={52} apiBaseUrl={apiBaseUrl} />
      <div className="flex min-w-0 flex-col">
        <h2 data-testid="general-headline" className="text-h3 truncate text-(--text)">
          {name === "" ? "Untitled project" : name}
        </h2>
        <p className="text-body flex items-center gap-(--space-2) text-(--text-muted)">
          <span data-testid="general-headline-identifier">{identifier === "" ? "No short code" : identifier}</span>
          <span aria-hidden="true">·</span>
          <span data-testid="general-network">{visibilityLabel(network)}</span>
          {created === "" ? null : (
            <>
              <span aria-hidden="true">·</span>
              <span data-testid="general-created">{created}</span>
            </>
          )}
        </p>
      </div>
      {canEdit ? (
        <div className="ml-auto flex shrink-0 items-center gap-(--space-2)">
          <Button variant="secondary" size="small" data-testid="general-emblem-open" onClick={onOpenMark}>
            <Smile size={14} strokeWidth={1.5} aria-hidden="true" />
            Change emblem
          </Button>
          <Button variant="secondary" size="small" data-testid="general-emblem-upload-open" onClick={onOpenUpload}>
            <ImageIcon size={14} strokeWidth={1.5} aria-hidden="true" />
            Upload image
          </Button>
        </div>
      ) : null}
    </section>
  );
}
