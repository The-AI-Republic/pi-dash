// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake addresses (parity/specs/intake.md INT-001, INT-002). Pure path
// builders: the canonical screen, the retired inbox address that
// forwards to it, and the feature-settings leaf the gate's shortcut
// targets. Keeping them in one place stops the screen watching a
// different address than the tab strip links to.

/** Canonical intake screen. */
export function intakeHref(workspaceSlug: string, projectId: string): string {
  return `/${workspaceSlug}/projects/${projectId}/intake`;
}

/** Retired project-inbox address; the route forwards it onto intake. */
export function retiredInboxHref(workspaceSlug: string, projectId: string): string {
  return `/${workspaceSlug}/projects/${projectId}/inbox`;
}

/**
 * The project's intake feature leaf in settings (INT-002). The shortcut
 * lands on the rendered leaf, not the bare `…/features` index, which has
 * no page of its own.
 */
export function intakeFeatureSettingsHref(workspaceSlug: string, projectId: string): string {
  return `/${workspaceSlug}/settings/projects/${projectId}/features/intake`;
}
