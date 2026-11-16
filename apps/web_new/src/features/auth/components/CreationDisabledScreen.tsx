// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Standalone creation-disabled screen (AUTH-041). When the instance
// config disables workspace creation, the standalone route renders this
// admin-only illustration plus go-back and ask-admin actions instead of
// the form; the ask-admin link opens an unaddressed mail draft with a
// prefilled subject and a body template signed by the requester. Written
// from parity/specs/auth.md.

import { Button } from "@pidash/kit";
import * as React from "react";

import { buildAdminRequestMailto } from "../workspace.js";

export const CREATION_DISABLED_HEADING = "Only the instance admin can create workspaces";
export const CREATION_DISABLED_BACK_LABEL = "Go back";
export const CREATION_DISABLED_ASK_LABEL = "Ask the instance admin";

export interface CreationDisabledScreenProps {
  requester: { firstName: string; lastName: string; email: string };
  onBack: () => void;
}

export function CreationDisabledScreen({ requester, onBack }: CreationDisabledScreenProps): React.ReactElement {
  return (
    <section aria-labelledby="creation-disabled-heading" className="flex flex-col items-center gap-(--space-4)">
      <svg width="96" height="96" viewBox="0 0 96 96" aria-hidden="true">
        <path
          d="M48 8 78 20v22c0 19-13 32-30 40C31 74 18 61 18 42V20L48 8Z"
          fill="none"
          stroke="var(--accent)"
          strokeWidth={4}
          strokeLinejoin="round"
        />
        <path
          d="M36 47l9 9 16-18"
          fill="none"
          stroke="var(--accent)"
          strokeWidth={4}
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      </svg>
      <h1 id="creation-disabled-heading" className="text-h2 text-center text-(--text)">
        {CREATION_DISABLED_HEADING}
      </h1>
      <p className="text-body text-center text-(--text-muted)">
        Workspace creation is turned off on this instance. If you know who administers it, send them a request — the
        draft is filled in for you.
      </p>
      <div className="mt-(--space-2) flex items-center gap-(--space-4)">
        <Button type="button" variant="primary" size="large" onClick={onBack}>
          {CREATION_DISABLED_BACK_LABEL}
        </Button>
        <a
          href={buildAdminRequestMailto(requester)}
          className="text-emphasis inline-flex h-(--control-height-large) cursor-pointer items-center justify-center gap-(--space-2) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-6) font-medium whitespace-nowrap text-(--text) transition-colors duration-(--motion-duration) hover:bg-(--subtle)"
        >
          {CREATION_DISABLED_ASK_LABEL}
        </a>
      </div>
    </section>
  );
}
