// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Setup card (parity/specs/agents.md §9, AGT-039). Replaces the chat entry
// on the landing and sits in a conversation's empty state until the user
// stores a provider key. The destination belongs to the profile area; the
// link crosses app boundaries until that area migrates.

import { cn } from "@pidash/kit";
import { Settings2 } from "lucide-react";
import * as React from "react";

import { CrossAppLink } from "../../../shared/coexistence/index.js";
import { PROVIDER_SETTINGS_HREF, SETUP_CARD_ACTION, SETUP_CARD_BODY, SETUP_CARD_TITLE } from "../lib/copy.js";

export interface SetupCardProps {
  className?: string;
}

export function SetupCard({ className }: SetupCardProps): React.ReactElement {
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
        to={PROVIDER_SETTINGS_HREF}
        role="button"
        className="text-body mt-(--space-2) inline-flex h-(--control-height) items-center rounded-(--radius-control) bg-(--accent) px-(--space-4) font-medium text-(--on-accent) hover:bg-(--accent-hover) focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-(--focus-ring-color)"
      >
        {SETUP_CARD_ACTION}
      </CrossAppLink>
    </section>
  );
}
