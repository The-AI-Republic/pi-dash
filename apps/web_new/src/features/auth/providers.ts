// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Third-party sign-in providers. The edition contributes the providers a
// build knows (cloud only; the OSS edition contributes none) and the
// instance config gates which of them are offered; only providers enabled
// in both are shown. Each resolves to a full-page navigation at the
// provider's native auth URL. Written from parity/specs/auth.md AUTH-003.

import type { InstanceConfig } from "@pidash/api-client";

import type { AuthProviderExtension } from "../../core/edition/types.js";
import type { AuthMode } from "./authErrors.js";

export interface SignInProvider {
  id: string;
  label: string;
  href: string;
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
