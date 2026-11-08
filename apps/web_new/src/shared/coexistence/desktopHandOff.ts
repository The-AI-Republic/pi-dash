// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Desktop browser hand-off (M-01/NEWFRONT-92). The desktop bundle ships
// only the screens apps/web_new owns. An address it does not own — the
// admin console, a public board, a screen the web proxy still serves from
// the old app — cannot render well inside the app window: those pages
// assume the hosted origin, and the native shell refuses to load a
// server-host page remotely. So the app window keeps the address it can
// render and hands every other one to the system browser at the same path
// on the hosted web app, where it would have landed on the web.
//
// The decision reads the same migrated-route list the proxy uses, so a
// screen joins the window the moment its route moves to apps/web_new.
// The web build never hands off: it already is the hosted app, and a link
// to an old-app screen there is a plain navigation (CrossAppLink).

import { parseHostedOrigin } from "../../core/platform/shareUrl.js";
import { isWebAddress } from "../../core/platform/urls.js";
import { isMigratedPath } from "./migrated-routes.js";

/** What the desktop shell should do with the address it is showing. */
export type DesktopRouteVerdict =
  /** apps/web_new renders this address; keep it in the window. */
  | { kind: "in-app" }
  /** Another frontend owns it: open the hosted equivalent instead. */
  | { kind: "hand-off"; path: string };

/**
 * Classify an address the desktop bundle is about to show.
 *
 * Anything the migrated-route list claims stays in the window. Everything
 * else belongs to another frontend and leaves for the browser at the same
 * path, query and fragment. The root address is always in-app: it is the
 * signed-out entry (and the edition's landing), never an old-app screen.
 */
export function classifyDesktopRoute(path: string, entries?: readonly string[]): DesktopRouteVerdict {
  if (isRootAddress(path)) return { kind: "in-app" };
  const migrated = entries === undefined ? isMigratedPath(path) : isMigratedPath(path, entries);
  return migrated ? { kind: "in-app" } : { kind: "hand-off", path };
}

/** A bare path of slashes, optionally followed by a query or fragment. */
const ROOT_PATH = /^\/+(?:[?#][\s\S]*)?$/;

/**
 * The root address (with any query or fragment) is the in-app entry. It is
 * also the entry when only a query or fragment is present, which is how a
 * redirect hands the address over before any path is written.
 */
function isRootAddress(path: string): boolean {
  if (path === "") return true;
  if (path.startsWith("?") || path.startsWith("#")) return true;
  return ROOT_PATH.test(path);
}

/**
 * Where the browser should go for a handed-off address: `path` resolved
 * against the configured hosted origin, or an absolute http(s) address
 * passed through unchanged (it already points at a browser).
 *
 * Throws rather than inventing an origin when none is configured. Bundled
 * pages run on a Tauri-owned origin, so falling back to the page origin
 * would hand the browser a `tauri://` address no browser can open.
 */
export function hostedAddressFor(path: string, webOrigin: string | undefined): string {
  if (isWebAddress(path)) return path;
  const origin = parseHostedOrigin(webOrigin);
  if (origin === undefined) {
    throw new Error("No hosted web app address is configured for this build.");
  }
  return new URL(path, `${origin}/`).toString();
}

/**
 * The address on screen, as the browser holds it: path plus query and
 * fragment. Both builds keep window.location in step with the route, so
 * this needs no router context (the not-found surface also renders in
 * the served 404 document, before any router exists).
 */
export function currentAddress(): string {
  const location = globalThis.window?.location;
  if (location === undefined) return "/";
  return `${location.pathname}${location.search}${location.hash}`;
}
