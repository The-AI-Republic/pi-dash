// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

export type PidashTarget = "web" | "desktop";

/** Build target for this bundle. Set at build time via PIDASH_TARGET. */
export const PIDASH_TARGET: PidashTarget =
  typeof __PIDASH_TARGET__ === "string" && __PIDASH_TARGET__ === "desktop" ? "desktop" : "web";

/** Parse a raw PIDASH_TARGET value (env, CLI) into a build target. */
export function resolvePidashTarget(raw: string | undefined): PidashTarget {
  return raw === "desktop" ? "desktop" : "web";
}
