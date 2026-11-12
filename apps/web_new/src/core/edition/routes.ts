// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Edition route + sidebar consumption (CLOUD-021/CLOUD-001–004). The cloud
// build declares its routes in `edition.routes` and their sidebar entries
// in `edition.sidebar`; the OSS module declares neither, so the sidebar
// renders unchanged and every gated route 404s or keeps its community
// behavior. Conventions (the private-pi-dash module contract, NEWFRONT-655
// and the NEWFRONT-1 decision of 2026-10-18):
//
// - Workspace routes carry the workspace-relative segment and omit `scope`
//   (the default): { path: "apps" } mounts at /<workspace>/apps/ and
//   resolves through isEditionRouteEnabled.
// - Top-level routes carry `scope: "top-level"` and the address without the
//   leading slash: { path: "home", scope: "top-level" } mounts at /home,
//   { path: "/", scope: "top-level" } declares the signed-out root
//   behavior, { path: "login", scope: "top-level" } serves the sign-in page
//   in place instead of forwarding it (CLOUD-002, NEWFRONT-645), and
//   isEditionPublicRouteEnabled answers for all of them.
// - Every top-level marketing address is also listed by PUBLIC_MARKETING_PATHS
//   so the prerender build, the sitemap and the shell's provider skip share
//   one source of truth. A new public page is added to that list and
//   declared by the cloud module; the OSS module declares none of them.
// - sidebar entries carry a workspace-relative target with a leading
//   slash, e.g. { id: "apps", label: "Apps", to: "/apps/" }, resolved
//   against the active workspace slug by resolveEditionSidebarHref.
import type { Edition, SettingsSection } from "./types.js";

/** Workspace route segment the cloud edition declares for the Apps catalog. */
export const APPS_CATALOG_ROUTE_PATH = "apps";

/** The signed-out root address a top-level edition route may claim. */
export const ROOT_ROUTE_PATH = "/";

/**
 * Public marketing addresses, in sitemap order (CLOUD-003–005). `/home`,
 * `/docs`, `/downloads` and `/pricing` are the framed marketing pages; the
 * sitemap lists the first, second and fourth (CLOUD-005) and the static
 * build prerenders all four plus the root.
 */
export const PUBLIC_MARKETING_PATHS = ["/home", "/docs", "/downloads", "/pricing"] as const;

/** The subset of the marketing pages crawlers are pointed at. */
export const SITEMAP_MARKETING_PATHS = ["/home", "/docs", "/pricing"] as const;

export type PublicMarketingPath = (typeof PUBLIC_MARKETING_PATHS)[number];

/** True for a route declared as a top-level (site-root) address. */
export function isTopLevelRoute(route: { path: string; scope?: "workspace" | "top-level" | undefined }): boolean {
  return route.scope === "top-level";
}

/** True when the edition declares the given workspace route segment. */
export function isEditionRouteEnabled(edition: Edition, routePath: string): boolean {
  return (edition.routes ?? []).some((route) => !isTopLevelRoute(route) && route.path === routePath);
}

/**
 * True when the edition declares the given top-level address. Accepts the
 * spelling used by the module ("home") and the address itself ("/home");
 * the root is always "/".
 */
export function isEditionPublicRouteEnabled(edition: Edition, path: string): boolean {
  const wanted = normalizeTopLevelPath(path);
  return (edition.routes ?? []).some((route) => isTopLevelRoute(route) && normalizeTopLevelPath(route.path) === wanted);
}

function normalizeTopLevelPath(path: string): string {
  if (path === ROOT_ROUTE_PATH) return ROOT_ROUTE_PATH;
  const trimmed = path.replace(/^\/+/, "").replace(/\/+$/, "");
  return trimmed === "" ? ROOT_ROUTE_PATH : `/${trimmed}`;
}

/**
 * The marketing page an edition route address names, ignoring a trailing
 * slash; null for anything outside the public marketing set.
 */
export function publicMarketingPathOf(pathname: string): PublicMarketingPath | null {
  const normalized = normalizeTopLevelPath(pathname);
  return (PUBLIC_MARKETING_PATHS as readonly string[]).includes(normalized)
    ? (normalized as PublicMarketingPath)
    : null;
}

/**
 * True when the address is one of the edition's public marketing pages.
 * Requires both the static set membership and the edition declaration, so
 * the OSS build never treats these paths as public.
 */
export function isEditionMarketingPath(edition: Edition, pathname: string): boolean {
  const marketing = publicMarketingPathOf(pathname);
  return marketing !== null && isEditionPublicRouteEnabled(edition, marketing);
}

/** True when the edition claims the signed-out root behavior. */
export function isEditionRootEnabled(edition: Edition): boolean {
  return isEditionPublicRouteEnabled(edition, ROOT_ROUTE_PATH);
}

/**
 * True when the app shell may skip the authenticated provider stack for
 * this address (CLOUD-005): the edition's marketing pages render from
 * props and read no session or query state. The root is intentionally not
 * included — its served document carries metadata, but hydration still
 * runs the redirected/gated tree.
 */
export function isEditionProviderFreePath(edition: Edition, pathname: string): boolean {
  return isEditionMarketingPath(edition, pathname);
}

/** Marketing home every public forward lands on (CLOUD-001). */
export const MARKETING_HOME_PATH = "/home";

/** Cloud sign-in page, rendered in place by the cloud edition (CLOUD-002). */
export const CLOUD_LOGIN_PATH = "/login";

/**
 * The address a signed-out visitor on the edition's root is sent to
 * (CLOUD-001). A safe return target is carried as the login page's
 * `next_path`, so the visitor comes back to it after signing in; an absent
 * or unsafe target is not carried, and the visitor lands on the marketing
 * home (CLOUD-013 discards unsafe values rather than forwarding them).
 * `safeNext` is the parser's already-sanitized value.
 */
export function signedOutRootTarget(safeNext: string | undefined): string {
  if (safeNext === undefined) return MARKETING_HOME_PATH;
  return `${CLOUD_LOGIN_PATH}?next_path=${encodeURIComponent(safeNext)}`;
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
