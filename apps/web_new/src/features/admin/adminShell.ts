// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dashboard shell pure logic (ADM-062, ADM-063, ADM-066, from
// parity/specs/admin.md §8): the sidebar entry table in render order,
// the prefix rule for the active highlight, the breadcrumb segment
// label map and trail builder, and the collapse persistence IO. Entry
// names, order, hrefs and crumb labels are pinned by the parity
// scenarios; descriptions and icons are fresh.
export const GOD_MODE_MOUNT = "/god-mode";

export interface AdminSidebarEntryDef {
  /** Stable key the sidebar maps to its icon. */
  key: string;
  /** Display name, pinned by the parity scenario. */
  name: string;
  /** Fresh one-line summary shown under the name. */
  description: string;
  /** Route slug under the mount. */
  slug: string;
  /** Console href in canonical trailing-slash form. */
  href: string;
}

/**
 * Sidebar entries in render order: general, email, authentication,
 * workspaces, AI, loop, runners. No image entry (removal target per
 * main#523); the runners entry sits last (target per main#534).
 */
export const ADMIN_SIDEBAR_ENTRIES: readonly AdminSidebarEntryDef[] = [
  {
    key: "general",
    name: "General",
    description: "Instance name, identity, and telemetry.",
    slug: "general",
    href: "/god-mode/general/",
  },
  {
    key: "email",
    name: "Email",
    description: "Outgoing mail server and test delivery.",
    slug: "email",
    href: "/god-mode/email/",
  },
  {
    key: "authentication",
    name: "Authentication",
    description: "Sign-in methods and provider credentials.",
    slug: "authentication",
    href: "/god-mode/authentication/",
  },
  {
    key: "workspace",
    name: "Workspaces",
    description: "Every workspace on this instance.",
    slug: "workspace",
    href: "/god-mode/workspace/",
  },
  {
    key: "ai",
    name: "Artificial intelligence",
    description: "Model and API key for AI features.",
    slug: "ai",
    href: "/god-mode/ai/",
  },
  {
    key: "loop",
    name: "Loop",
    description: "Recurring background jobs and their runs.",
    slug: "loop",
    href: "/god-mode/loop/",
  },
  {
    key: "runners",
    name: "Runners",
    description: "Machines running agents for this instance.",
    slug: "runners",
    href: "/god-mode/runners/",
  },
];

/** Trailing slashes trimmed for comparison; the root keeps its own. */
function stripTrailingSlash(path: string): string {
  const bare = path.replace(/\/+$/, "");
  return bare === "" ? "/" : bare;
}

/**
 * Whether the entry carries the active highlight on `pathname`: the
 * entry matches exactly or by path prefix, so a provider sub-page
 * keeps its authentication parent highlighted. Segment-aware, so
 * `/god-mode/ai-extra/` never lights the AI entry.
 */
export function isSidebarEntryActive(pathname: string, href: string): boolean {
  const current = stripTrailingSlash(pathname);
  const target = stripTrailingSlash(href);
  return current === target || current.startsWith(`${target}/`);
}

// --- Breadcrumbs (ADM-066) -------------------------------------------------

/** Leading crumb: always back to general settings. */
export const ADMIN_CRUMB_HOME = { label: "Settings", href: "/god-mode/general/" } as const;

export interface AdminCrumb {
  label: string;
  href: string;
}

/**
 * Fixed segment label map. Unmapped segments (loop, runners, raw ids)
 * render uppercased. There is no image entry: the console offers no
 * image settings area, so `/god-mode/image/` never reaches this map —
 * it falls through to the not-found screen (ADM-049).
 */
export const ADMIN_SEGMENT_LABELS: Readonly<Record<string, string>> = {
  general: "General",
  ai: "Artificial Intelligence",
  email: "Email",
  authentication: "Authentication",
  google: "Google",
  github: "GitHub",
  gitlab: "GitLab",
  gitea: "Gitea",
  workspace: "Workspace",
  create: "Create",
};

/** Label for one segment: the mapped name, else uppercased. */
export function adminSegmentLabel(segment: string): string {
  return ADMIN_SEGMENT_LABELS[segment] ?? segment.toUpperCase();
}

/**
 * The header trail for a console address: the settings lead plus one
 * crumb per mount-relative segment, each linking to its cumulative
 * path in canonical trailing-slash form.
 */
export function buildAdminCrumbs(pathname: string): AdminCrumb[] {
  const segments = pathname.split("/").filter((part) => part !== "");
  const relative = segments[0] === "god-mode" ? segments.slice(1) : segments;
  const crumbs: AdminCrumb[] = [{ label: ADMIN_CRUMB_HOME.label, href: ADMIN_CRUMB_HOME.href }];
  let cumulative = GOD_MODE_MOUNT;
  for (const segment of relative) {
    cumulative += `/${segment}`;
    crumbs.push({ label: adminSegmentLabel(segment), href: `${cumulative}/` });
  }
  return crumbs;
}

// --- Collapse persistence (ADM-063) ------------------------------------------

/** Viewport at or below this width counts as narrow (mobile behavior). */
export const ADMIN_NARROW_VIEWPORT_PX = 768;

/** Raw storage key for the collapsed flag. Pinned by the parity driver. */
export const ADMIN_SIDEBAR_COLLAPSED_KEY = "god_mode_sidebar_collapsed";

function collapseStore(): Pick<Storage, "getItem" | "setItem"> | null {
  return globalThis.window?.localStorage ?? null;
}

/** The persisted collapsed flag; absent or unknown reads expanded. */
export function readStoredSidebarCollapsed(): boolean {
  return collapseStore()?.getItem(ADMIN_SIDEBAR_COLLAPSED_KEY) === "true";
}

/** Persist the collapsed flag; no store, no-op. */
export function writeStoredSidebarCollapsed(collapsed: boolean): void {
  collapseStore()?.setItem(ADMIN_SIDEBAR_COLLAPSED_KEY, collapsed ? "true" : "false");
}
