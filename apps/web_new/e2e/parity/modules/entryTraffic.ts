// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The modules list's traffic rules (parity/specs/modules.md §11.3–§11.4,
// §13, MOD-042, MOD-043). Two separate checks, because the list sits
// inside a frame that legitimately reads project and workspace state:
//
// 1. Every request that names module traffic must be one of the module
//    endpoints the spec names. Anything else is a module surface inventing
//    an endpoint.
// 2. No request at all — module-owned or frame-owned — may name a desktop
//    shell, a license, a plan or a purchase. That is what makes "no
//    desktop-only and no edition-only module path" observable: an
//    edition-gated module surface would reach one of those endpoints on
//    entry, and the driver returns every API path so it reaches this check
//    instead of being filtered out by the spy.
//
// The frame's own reads (project row, roles, members, favorites,
// notifications, preferences) are not module traffic and are not this
// area's to police; the module check stays on what names modules.

/** The module endpoints a module screen may read (§13). */
const MODULE_ENDPOINTS: readonly RegExp[] = Object.freeze([
  /\/modules\/?$/,
  /\/modules\/[0-9a-f-]+\/?$/,
  /archived-modules/,
  /module-links/,
  /user-favorite-modules/,
  /modules\/[0-9a-f-]+\/issues/,
]);

/** Whether a path names module traffic, whatever endpoint it actually is. */
const NAMES_MODULES = /modules?([/-]|$)/i;

/**
 * Path segments that name a desktop shell, a license, a plan or a
 * purchase. A module entry reaching any of them means a surface is gated
 * by the build or the edition.
 */
const DESKTOP_OR_EDITION: RegExp =
  /(^|[/_-])(desktop|tauri|electron|licenses?|billing|subscriptions?|premium|upgrade|plans?|payments?|checkout)([/_-]|$)/i;

/** Whether a request path names a desktop-shell, license or plan surface. */
export function isDesktopOrEditionPath(path: string): boolean {
  return DESKTOP_OR_EDITION.test(path);
}

/** Whether a path names modules at all (module endpoints or invented ones). */
export function namesModuleTraffic(path: string): boolean {
  return NAMES_MODULES.test(path);
}

/** Whether a module-naming path is one of the spec's module endpoints. */
export function isModuleEndpointPath(path: string): boolean {
  return MODULE_ENDPOINTS.some((pattern) => pattern.test(path));
}

export interface EntryTrafficViolations {
  /** Requests naming a desktop shell, license or plan; must be empty. */
  prohibited: string[];
  /** Requests that name modules but are not a known module endpoint. */
  unknownModule: string[];
}

/** Both failures over one list entry's observed API paths. */
export function entryTrafficViolations(paths: readonly string[]): EntryTrafficViolations {
  return {
    prohibited: paths.filter((path) => isDesktopOrEditionPath(path)),
    unknownModule: paths.filter((path) => namesModuleTraffic(path) && !isModuleEndpointPath(path)),
  };
}
