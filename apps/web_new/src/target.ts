// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).

export type PidashTarget = "web" | "desktop";

/** Build target for this bundle. Set at build time via PIDASH_TARGET. */
export const PIDASH_TARGET: PidashTarget =
  typeof __PIDASH_TARGET__ === "string" && __PIDASH_TARGET__ === "desktop" ? "desktop" : "web";

/** Parse a raw PIDASH_TARGET value (env, CLI) into a build target. */
export function resolvePidashTarget(raw: string | undefined): PidashTarget {
  return raw === "desktop" ? "desktop" : "web";
}
