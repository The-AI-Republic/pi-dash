// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Edition route + sidebar consumption (CLOUD-021, from
// parity/specs/cloud.md §5). The cloud build declares its workspace
// routes in `edition.routes` and their sidebar entries in
// `edition.sidebar`; the OSS module declares neither, so the sidebar
// renders byte-identical and the route 404s. Conventions (the
// private-pi-dash module contract, NEWFRONT-655):
// - routes carry the workspace-relative segment, e.g. { path: "apps" };
// - sidebar entries carry a workspace-relative target with a leading
//   slash, e.g. { id: "apps", label: "Apps", to: "/apps/" }, resolved
//   against the active workspace slug by resolveEditionSidebarHref.
import type { Edition, SettingsSection } from "./types.js";

/** Workspace route segment the cloud edition declares for the Apps catalog. */
export const APPS_CATALOG_ROUTE_PATH = "apps";

/** True when the edition declares the given workspace route segment. */
export function isEditionRouteEnabled(edition: Edition, routePath: string): boolean {
  return (edition.routes ?? []).some((route) => route.path === routePath);
}

/**
 * True when the edition declares an extra settings section with this id.
 * The settings shell reads the sections once and lets each declared id
 * replace the edition-neutral body with the edition's own screen
 * (CLOUD-032, from the maintainer decision of 2026-10-18).
 */
export function isEditionSettingsSectionEnabled(edition: Edition, sectionId: string): boolean {
  return (edition.settingsSections ?? []).some((section) => section.id === sectionId);
}

/** The declared section for an id, when the edition contributes one. */
export function findEditionSettingsSection(edition: Edition, sectionId: string): SettingsSection | null {
  return (edition.settingsSections ?? []).find((section) => section.id === sectionId) ?? null;
}

/**
 * Resolve an edition sidebar target against a workspace slug. A missing
 * leading slash is forgiven; the trailing slash rides through from the
 * module so workspace links keep the app's trailing-slash shape.
 */
export function resolveEditionSidebarHref(workspaceSlug: string, to: string): string {
  const target = to.startsWith("/") ? to : `/${to}`;
  return `/${workspaceSlug}${target}`;
}
