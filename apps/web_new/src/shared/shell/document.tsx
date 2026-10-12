// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Document shell runtime (SHELL-107, from parity/specs/shell.md §1). Two
// layers: the served index.html carries the crawler-facing markers, and
// once the app hydrates each route owns the tab title plus crawler
// directives while the static markers (icons, manifests, portal roots,
// theme mark) stay put. No third-party recorder is loaded here (D14).

import { applyTheme, resolveTheme } from "@pidash/kit";
import * as React from "react";
import { createPortal } from "react-dom";

/** Static overlay roots served by index.html, above the provider tree. */
export const MENU_PORTAL_ID = "context-menu-portal";
export const EDITOR_PORTAL_ID = "editor-portal";
export const FULL_SCREEN_PORTAL_ID = "full-screen-portal";

export type OverlayRootId = typeof MENU_PORTAL_ID | typeof EDITOR_PORTAL_ID | typeof FULL_SCREEN_PORTAL_ID;

/** Served marketing tags the hydrated route pair replaces (title + robots). */
const SERVED_MARKETING_SELECTOR = 'meta[name="description"], meta[property^="og:"], meta[name^="twitter:"]';

function stripServedMarketingTags(): void {
  for (const tag of [...document.querySelectorAll(SERVED_MARKETING_SELECTOR)]) {
    tag.remove();
  }
}

export interface DocumentHead {
  /** Tab title while the route shows. */
  title: string;
  /** Crawler directives while the route shows. */
  robots: string;
}

/**
 * Own the tab title and crawler directives while mounted, restoring the
 * previous pair on unmount. The first mount also swaps the served product
 * metadata out so hydrated routes never double-report it.
 */
export function useDocumentHead({ title, robots }: DocumentHead): void {
  React.useEffect(() => {
    const previousTitle = document.title;
    document.title = title;

    let robotsTag = document.querySelector<HTMLMetaElement>('meta[name="robots"]');
    const created = robotsTag === null;
    const previousRobots = robotsTag?.getAttribute("content") ?? null;
    if (robotsTag === null) {
      robotsTag = document.createElement("meta");
      robotsTag.setAttribute("name", "robots");
      document.head.appendChild(robotsTag);
    }
    robotsTag.setAttribute("content", robots);

    stripServedMarketingTags();

    const current = robotsTag;
    return () => {
      document.title = previousTitle;
      if (created) {
        current.remove();
      } else if (previousRobots === null) {
        current.removeAttribute("content");
      } else {
        current.setAttribute("content", previousRobots);
      }
    };
  }, [title, robots]);
}

/**
 * Mark the root with the resolved initial color scheme (kit default: follow
 * the OS). Runs once at boot; theme preference switching belongs to the
 * appearance slice, which reuses this mark.
 */
export function markInitialColorScheme(): "light" | "dark" {
  const scheme = resolveTheme("system");
  if (typeof document === "undefined") return scheme;
  document.documentElement.style.setProperty("color-scheme", scheme);
  applyTheme(scheme);
  return scheme;
}

export interface OverlayPortalProps {
  rootId: OverlayRootId;
  children: React.ReactNode;
}

/** Render children into one of the static overlay roots (menus, editors). */
export function OverlayPortal({ rootId, children }: OverlayPortalProps): React.ReactNode {
  if (typeof document === "undefined") return null;
  const host = document.getElementById(rootId);
  if (host === null) return null;
  return createPortal(children, host);
}
