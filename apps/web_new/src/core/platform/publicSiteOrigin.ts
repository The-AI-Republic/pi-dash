// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public site origin for crawlable marketing metadata (CLOUD-005). The
// static build bakes `PIDASH_PUBLIC_SITE_ORIGIN` into the bundle as the
// one define below, and the prerender step reads the same variable, so the
// served document and the hydrated head always name the same address. A
// build without it uses the hosted product address.

/** Product address a build targets when the environment names none. */
export const DEFAULT_PUBLIC_SITE_ORIGIN = "https://pidash.airepublic.com";

/** Process-like environment lookup. */
export type PublicSiteOriginEnv = Readonly<Record<string, string | undefined>>;

/** Configured origin without trailing slashes; blank counts as absent. */
export function publicSiteOriginFromEnv(env: PublicSiteOriginEnv): string {
  const configured = env.PIDASH_PUBLIC_SITE_ORIGIN;
  const value = typeof configured === "string" ? configured.trim() : "";
  return (value === "" ? DEFAULT_PUBLIC_SITE_ORIGIN : value).replace(/\/+$/, "");
}

/** The origin the current bundle was compiled for. */
declare const __PIDASH_PUBLIC_SITE_ORIGIN__: string | undefined;

/**
 * The origin this build serves from. Vite replaces the define at build
 * time; a bare test run (no define) falls back to the default, and tests
 * can stub the global to prove a deployment origin survives hydration.
 */
export function bakedPublicSiteOrigin(): string {
  return publicSiteOriginFromEnv({
    PIDASH_PUBLIC_SITE_ORIGIN:
      typeof __PIDASH_PUBLIC_SITE_ORIGIN__ === "string" ? __PIDASH_PUBLIC_SITE_ORIGIN__ : undefined,
  });
}
