// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Build-time choice of the hosted app origin (DESK-019). The desktop
// pipeline bakes `VITE_WEB_BASE_URL`; newer tooling may pass the same value
// as `PIDASH_WEB_BASE_URL`. Either name feeds the one build define the
// runtime reads, and an empty value counts as absent so a blank variable
// never masks a real one.

/** Process-like environment lookup: either name may be absent. */
export type HostedBaseUrlEnv = Readonly<Record<string, string | undefined>>;

/** First configured, non-blank hosted base, or null when none is set. */
export function hostedBaseUrlFromEnv(env: HostedBaseUrlEnv): string | null {
  for (const candidate of [env.PIDASH_WEB_BASE_URL, env.VITE_WEB_BASE_URL]) {
    if (typeof candidate === "string" && candidate.trim() !== "") return candidate;
  }
  return null;
}
