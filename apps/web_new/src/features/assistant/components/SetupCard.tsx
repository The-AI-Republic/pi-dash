// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Setup card (parity/specs/agents.md §9, AGT-039). Replaces the chat entry
// on the landing and sits in a conversation's empty state until the user
// stores a provider key. The destination is the account settings page of
// the workspace the viewer is in (main#670), so the card takes its slug.

import { cn } from "@pidash/kit";
import { Settings2 } from "lucide-react";
import * as React from "react";

import { CrossAppLink } from "../../../shared/coexistence/index.js";
import { providerSettingsHref, SETUP_CARD_ACTION, SETUP_CARD_BODY, SETUP_CARD_TITLE } from "../lib/copy.js";

export interface SetupCardProps {
  /** Workspace whose account settings hold the provider key (main#670). */
  workspaceSlug: string;
  className?: string;
}

export function SetupCard({ workspaceSlug, className }: SetupCardProps): React.ReactElement {
  return (
    <section
      aria-label={SETUP_CARD_TITLE}
      data-testid="assistant-setup-card"
      className={cn(
        "mx-auto flex w-full max-w-md flex-col items-center gap-(--space-3) rounded-(--radius-dialog) border border-(--border) bg-(--surface) px-(--space-6) py-(--space-8) text-center",
        className
      )}
    >
      <Settings2 aria-hidden="true" size={22} className="text-(--accent)" />
      <h2 className="text-h3 text-(--text)">{SETUP_CARD_TITLE}</h2>
      <p className="text-body text-(--text-muted)">{SETUP_CARD_BODY}</p>
      <CrossAppLink
        to={providerSettingsHref(workspaceSlug)}
        role="button"
        className="text-body mt-(--space-2) inline-flex h-(--control-height) items-center rounded-(--radius-control) bg-(--accent) px-(--space-4) font-medium text-(--on-accent) hover:bg-(--accent-hover) focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-(--focus-ring-color)"
      >
        {SETUP_CARD_ACTION}
      </CrossAppLink>
    </section>
  );
}
