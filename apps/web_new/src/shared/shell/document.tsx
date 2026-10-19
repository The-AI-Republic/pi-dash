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

/**
 * Served marketing tags the hydrated route pair replaces (title + robots):
 * description, social preview data and the prerendered canonical. The
 * route re-creates all of them from its own claim, so leaving the served
 * copy behind would report two canonical addresses for one page.
 */
const SERVED_MARKETING_SELECTOR =
  'meta[name="description"], meta[property^="og:"], meta[name^="twitter:"], link[rel="canonical"]';

/** Tags a marketing route (re)creates alongside title + robots. */
const MANAGED_TAG_ATTRIBUTE = "data-route-head";

/** One head tag a crawlable route adds alongside title and robots. */
export interface HeadTag {
  tag: "meta" | "link";
  attributes: Readonly<Record<string, string>>;
}

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
  /** Canonical address; present only on crawlable marketing pages. */
  canonical?: string;
  /** Extra crawl/social tags; present only on crawlable marketing pages. */
  tags?: readonly HeadTag[];
  /**
   * Fallback owners (the app root's product default) lose to every
   * route-level owner no matter the effect order. Parent effects run
   * after child effects, so without this the root default always
   * clobbers route titles whenever their data is already cached.
   */
  fallback?: boolean;
}

interface HeadClaim {
  id: number;
  fallback: boolean;
  title: string;
  robots: string;
  tags: readonly HeadTag[] | undefined;
}

let nextClaimId = 0;
let claims: HeadClaim[] = [];
let bootTitle = "";
let bootRobotsPresent = false;
let bootRobotsContent: string | null = null;

function captureBootHead(): void {
  bootTitle = document.title;
  const tag = document.querySelector<HTMLMetaElement>('meta[name="robots"]');
  bootRobotsPresent = tag !== null;
  bootRobotsContent = tag?.getAttribute("content") ?? null;
}

function topClaim(): HeadClaim | undefined {
  // Route owners beat fallbacks; among equals the most recently
  // mounted wins, so a freshly mounted route replaces the previous
  // one even if the old tree lingers for a commit.
  let top: HeadClaim | undefined;
  for (const claim of claims) {
    if (top === undefined || (top.fallback && !claim.fallback)) {
      top = claim;
    } else if (top.fallback === claim.fallback && claim.id > top.id) {
      top = claim;
    }
  }
  return top;
}

function removeManagedTags(): void {
  for (const tag of [...document.querySelectorAll(`[${MANAGED_TAG_ATTRIBUTE}]`)]) {
    tag.remove();
  }
}

function appendManagedTag(
  tagName: "meta" | "link",
  attributes: Readonly<Record<string, string>>
): HTMLMetaElement | HTMLLinkElement {
  const tag = document.createElement(tagName);
  for (const [name, value] of Object.entries(attributes)) tag.setAttribute(name, value);
  tag.setAttribute(MANAGED_TAG_ATTRIBUTE, "");
  document.head.appendChild(tag);
  return tag;
}

/** The owning route's crawl/social tags, layered on its title and robots. */
function applyManagedTags(claim: HeadClaim): void {
  for (const tag of claim.tags ?? []) appendManagedTag(tag.tag, tag.attributes);
}

function applyHeadClaim(claim: HeadClaim | undefined): void {
  stripServedMarketingTags();
  removeManagedTags();
  if (claim === undefined) {
    document.title = bootTitle;
    const tag = document.querySelector<HTMLMetaElement>('meta[name="robots"]');
    if (!bootRobotsPresent) {
      tag?.remove();
    } else if (tag !== null) {
      if (bootRobotsContent === null) {
        tag.removeAttribute("content");
      } else {
        tag.setAttribute("content", bootRobotsContent);
      }
    }
    return;
  }
  document.title = claim.title;
  let tag = document.querySelector<HTMLMetaElement>('meta[name="robots"]');
  if (tag === null) {
    tag = document.createElement("meta");
    tag.setAttribute("name", "robots");
    document.head.appendChild(tag);
  }
  tag.setAttribute("content", claim.robots);
  applyManagedTags(claim);
}

/**
 * Own the tab title and crawler directives while mounted. Nested owners
 * stack: a route title beats the root fallback, and unmounting restores
 * the surviving owner (or the served head when none remains). Each
 * application also swaps the served product metadata out so hydrated
 * routes never double-report it.
 */
export function useDocumentHead({ title, robots, fallback = false, tags }: DocumentHead): void {
  const claimRef = React.useRef<HeadClaim | null>(null);
  if (claimRef.current === null) {
    nextClaimId += 1;
    claimRef.current = { id: nextClaimId, fallback, title, robots, tags };
  }
  React.useEffect(() => {
    const claim = claimRef.current;
    if (claim === null) return;
    // A fresh generation (nothing mounted) recaptures the served head,
    // so test renders and full remounts restore what they found.
    if (claims.length === 0) captureBootHead();
    claim.title = title;
    claim.robots = robots;
    claim.fallback = fallback;
    claim.tags = tags;
    if (!claims.includes(claim)) claims.push(claim);
    applyHeadClaim(topClaim());
    return () => {
      claims = claims.filter((entry) => entry !== claim);
      applyHeadClaim(topClaim());
    };
  }, [title, robots, fallback, tags]);
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
