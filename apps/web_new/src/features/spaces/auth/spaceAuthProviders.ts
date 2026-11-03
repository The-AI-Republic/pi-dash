// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Third-party sign-in buttons for the spaces auth card
// (parity/specs/space.md SPC-062). A button renders exactly when the
// instance enables its provider; activating one starts the provider flow
// at the workspace provider path carrying the encoded return address.
// The compact monograms are original glyphs: brand artwork stays with
// the old app, which is reference-only.
import type { SpaceAuthMode } from "./spaceAuthMachine.js";

export type SpaceAuthProviderId = "google" | "github" | "gitlab" | "gitea";

export interface SpaceAuthProviderFlags {
  googleEnabled: boolean;
  githubEnabled: boolean;
  gitlabEnabled: boolean;
  giteaEnabled: boolean;
}

export interface SpaceAuthProvider {
  id: SpaceAuthProviderId;
  name: string;
  /** Two-letter monogram for the compact button. */
  monogram: string;
  /** Full invitation naming the provider; follows the mode. */
  label: string;
  /** Provider flow start, carrying the return address when handed one. */
  href: string;
}

const KNOWN_PROVIDERS: { id: SpaceAuthProviderId; name: string; monogram: string }[] = [
  { id: "google", name: "Google", monogram: "GO" },
  { id: "github", name: "GitHub", monogram: "GH" },
  { id: "gitlab", name: "GitLab", monogram: "GL" },
  { id: "gitea", name: "Gitea", monogram: "GT" },
];

/**
 * Resolve the enabled providers in display order. Unknown flags never
 * appear; with none enabled the result is empty and the card renders no
 * buttons and no divider slot.
 */
export function resolveSpaceAuthProviders(
  flags: SpaceAuthProviderFlags,
  mode: SpaceAuthMode,
  nextPath?: string | undefined
): SpaceAuthProvider[] {
  const enabled: Record<SpaceAuthProviderId, boolean> = {
    google: flags.googleEnabled,
    github: flags.githubEnabled,
    gitlab: flags.gitlabEnabled,
    gitea: flags.giteaEnabled,
  };
  const invitation = mode === "sign-up" ? "Create your account with" : "Continue with";
  const suffix = nextPath === undefined || nextPath === "" ? "" : `?next_path=${encodeURIComponent(nextPath)}`;
  return KNOWN_PROVIDERS.filter((provider) => enabled[provider.id]).map((provider) => ({
    ...provider,
    label: `${invitation} ${provider.name}`,
    href: `/auth/${provider.id}/${suffix}`,
  }));
}
