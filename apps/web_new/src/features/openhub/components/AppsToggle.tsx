// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Agent-tools master switch (written from parity/specs/cloud.md §5,
// CLOUD-029). The control states the whole privilege boundary up front:
// enabling exposes every installed app's tools — including ones that
// create, change or delete data — to every agent run the viewer starts.
// Persistence (optimistic with rollback) lives in api/mutations.ts.
import { Switch } from "@pidash/kit";
import * as React from "react";

/** Accessible name of the switch. */
export const AGENT_TOOLS_LABEL = "Let agents use installed apps";

/**
 * The privilege boundary, in full. The oracle pins the agents/tools
 * wording plus the create/delete scope (apps-toggle.spec.ts).
 */
export const AGENT_TOOLS_DISCLOSURE =
  "When this is on, every agent run you start can call tools from every app you install — " +
  "including tools that create, change, or delete data in those services. " +
  "When it is off, agents can only use Pi Dash's own tools.";

export interface AppsToggleProps {
  /** Current flag value. */
  enabled: boolean;
  /** True while a write is in flight: the control stays inert. */
  disabled: boolean;
  /** Persist one flip. */
  onToggle: (enabled: boolean) => void;
}

export function AppsToggle({ enabled, disabled, onToggle }: AppsToggleProps): React.ReactElement {
  return (
    <section aria-label="Agent tools" className="flex items-start justify-between gap-(--space-4)">
      <p className="text-body text-(--text-muted)">{AGENT_TOOLS_DISCLOSURE}</p>
      <Switch label={AGENT_TOOLS_LABEL} checked={enabled} disabled={disabled} onCheckedChange={onToggle} />
    </section>
  );
}
