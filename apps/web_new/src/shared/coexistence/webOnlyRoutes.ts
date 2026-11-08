// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Screens that exist only in the web build (desktop spec §11, decision
// D13). The instance console and the public boards are mounted from the
// same route tree as everything else, but only under the web target: a
// desktop build carries neither surface. Their addresses therefore count
// as "another frontend owns this" even though the migrated-route list
// claims them for the hosted app.

import list from "../../../migrated-routes.json";
import { isMigratedPath } from "./migrated-routes.js";

/** Route subtrees the desktop bundle never mounts. */
export const WEB_ONLY_DESKTOP_EXCLUSIONS: readonly string[] = ["/god-mode", "/spaces"];

/**
 * Throw unless every excluded subtree is a migrated-route entry.
 *
 * The exclusion is a subset of the hosted list: a screen that is not in the
 * list already hands off, so listing it as web-only would change nothing
 * and instead suggest the bundle carries it. Checked at import time so a
 * typo or a removed entry fails loudly rather than silently starting to
 * render natively.
 */
export function assertExclusionsAreMigrated(
  exclusions: readonly string[] = WEB_ONLY_DESKTOP_EXCLUSIONS,
  entries: readonly string[] = list.entries
): void {
  const migrated = new Set(entries);
  const missing = exclusions.find((path) => !migrated.has(path));
  if (missing !== undefined) {
    throw new Error(`web-only desktop exclusion ${JSON.stringify(missing)} is not in migrated-routes.json`);
  }
}

assertExclusionsAreMigrated();

/**
 * True when the address belongs to a web-only subtree. Matched with the
 * migrated-route rules, so `/god-mode` covers everything below it and
 * `/spaces/` with a trailing slash is the same address.
 */
export function isWebOnlyDesktopPath(path: string): boolean {
  return isMigratedPath(path, WEB_ONLY_DESKTOP_EXCLUSIONS, []);
}
