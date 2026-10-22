// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Third-party sign-in providers. The edition contributes the providers a
// build knows (OSS lists the four native ones; cloud adds its extras) and
// the instance config gates which of them are offered; only providers
// enabled in both are shown. Each resolves to a full-page navigation at
// the provider's native auth URL. Written from parity/specs/auth.md AUTH-003.
// The cloud edition's organizational provider (CLOUD-017, from
// parity/specs/cloud.md §4) resolves below it: no instance flag gates it,
// so listing it in the edition is the only gate (OSS builds never list
// it). Its button carries the current page through the round trip.

import { cloudSignInHandoffUrl, type InstanceConfig } from "@pidash/api-client";

import type { AuthProviderExtension } from "../../core/edition/types.js";
import type { AuthMode } from "./authErrors.js";

/**
 * The organizational provider id the cloud edition lists in `auth`
 * (CLOUD-017). The id is stable: the cloud module and this resolver must
 * agree on it, and a rename breaks the provider button.
 */
export const CLOUD_PROVIDER_ID = "airepublic";

const CLOUD_PROVIDER_NAME = "AI Republic";

export interface SignInProvider {
  id: string;
  label: string;
  href: string;
  /**
   * Cloud handoff details, present only for the organizational provider.
   * The card clicks through the desktop-aware handoff with this return
   * target instead of navigating `href` blindly.
   */
  cloudHandoff?: { returnTo: string } | undefined;
}

interface KnownProvider {
  flag: keyof Pick<
    InstanceConfig,
    "is_google_enabled" | "is_github_enabled" | "is_gitlab_enabled" | "is_gitea_enabled"
  >;
  path: string;
  name: string;
}

const KNOWN_PROVIDERS: Record<string, KnownProvider> = {
  google: { flag: "is_google_enabled", path: "google", name: "Google" },
  github: { flag: "is_github_enabled", path: "github", name: "GitHub" },
  gitlab: { flag: "is_gitlab_enabled", path: "gitlab", name: "GitLab" },
  gitea: { flag: "is_gitea_enabled", path: "gitea", name: "Gitea" },
};

export interface ResolveProvidersOptions {
  mode: AuthMode;
  /** API origin the native auth URLs live on. */
  apiOrigin: string;
  nextPath?: string | undefined;
  /** Current page (path plus query, never the fragment) the cloud handoff carries back to. */
  returnToPage?: string | undefined;
  /** Browser origin, sent only when it differs from the auth origin. */
  browserOrigin?: string | undefined;
}

/** Intersect the edition's providers with the instance flags; unknown ids are ignored. */
export function resolveSignInProviders(
  editionProviders: AuthProviderExtension[] | undefined,
  config: InstanceConfig,
  options: ResolveProvidersOptions
): SignInProvider[] {
  const origin = options.apiOrigin.replace(/\/+$/, "");
  const verb = options.mode === "sign-up" ? "Sign up" : "Sign in";
  const resolved: SignInProvider[] = [];
  for (const provider of editionProviders ?? []) {
    if (provider.id === CLOUD_PROVIDER_ID) {
      const returnTo = options.returnToPage ?? "/";
      resolved.push({
        id: provider.id,
        label: `${verb} with ${CLOUD_PROVIDER_NAME}`,
        href: cloudSignInHandoffUrl(options.apiOrigin, {
          returnTo,
          ...(options.browserOrigin === undefined ? {} : { returnOrigin: options.browserOrigin }),
        }),
        cloudHandoff: { returnTo },
      });
      continue;
    }
    const known = KNOWN_PROVIDERS[provider.id];
    if (known === undefined || !config[known.flag]) continue;
    const href =
      options.nextPath === undefined
        ? `${origin}/auth/${known.path}/`
        : `${origin}/auth/${known.path}/?next_path=${encodeURIComponent(options.nextPath)}`;
    resolved.push({ id: provider.id, label: `${verb} with ${known.name}`, href });
  }
  return resolved;
}
