// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Per-provider credential field sets (ADM-028–031, from
// parity/specs/admin.md §4.6–4.9): the inputs each provider form renders
// through the shared shell, the Gitea public-host prefill, and the
// copy-field paste guidance with provider-console help links. Pinned
// values (config keys, placeholders, the Gitea default host, help href
// shapes) match the parity scenarios; every word around them is fresh.
import * as React from "react";

import type { AuthProviderSlug, CopyHelpHref, ProviderCredentialField } from "./providerAuth.js";

/** Public GitLab service address, pinned as the GitLab host placeholder. */
export const GITLAB_PUBLIC_HOST = "https://gitlab.com";

/** Public Gitea service address: the unset-host default and placeholder. */
export const GITEA_PUBLIC_HOST = "https://gitea.com";

/** Where a GitHub OAuth application is registered. */
export const GITHUB_OAUTH_CONSOLE_URL = "https://github.com/settings/applications/new";

/** GitLab's OAuth provider guide. */
export const GITLAB_OAUTH_DOCS_URL = "https://docs.gitlab.com/ee/integration/oauth_provider.html";

/** Google's guide to creating OAuth client credentials. */
export const GOOGLE_CLIENT_GUIDE_URL =
  "https://developers.google.com/identity/protocols/oauth2/javascript-implicit-flow#creatingcred";

/** Google's guide to finding an OAuth client's secret. */
export const GOOGLE_SECRET_GUIDE_URL =
  "https://developers.google.com/identity/oauth2/web/guides/get-google-api-clientid";

/** Where a Google OAuth client is administered. */
export const GOOGLE_OAUTH_CONSOLE_URL = "https://console.cloud.google.com/apis/credentials/oauthclient";

/** The public service's OAuth application settings, for Gitea field help. */
export const GITEA_PUBLIC_APPLICATIONS_URL = "https://gitea.com/user/settings/applications";

/** Configuration key carrying the Gitea host. */
export const GITEA_HOST_KEY = "GITEA_HOST";

/**
 * Stored-value prefill for the Gitea form: a blank stored host shows the
 * public service, everything else passes through untouched.
 */
export function giteaHostPrefill(key: string, stored: string): string {
  if (key === GITEA_HOST_KEY && stored === "") return GITEA_PUBLIC_HOST;
  return stored;
}

/**
 * Gitea application-settings link for the live credential draft: the
 * typed host wins, and a cleared host falls back to the public service
 * (the NEWFRONT-492 intended behavior — the link never lags the draft).
 */
export function giteaApplicationsHref(fieldValues: Record<string, string>): string {
  const typed = (fieldValues[GITEA_HOST_KEY] ?? "").trim();
  const base = typed === "" ? GITEA_PUBLIC_HOST : typed.replace(/\/+$/, "");
  return `${base}/user/settings/applications`;
}

/** Provider-console link inside a credential field's help copy. */
function ConsoleLink({ href, children }: { href: string; children: React.ReactNode }): React.ReactElement {
  return (
    <a href={href} target="_blank" rel="noreferrer" className="font-medium text-(--accent) hover:underline">
      {children}
    </a>
  );
}

/** Copy-field help link: static, or resolved from the live draft. */
export interface ProviderConfigCopyHelp {
  readonly href: CopyHelpHref;
  readonly label: string;
}

/** Everything one provider route passes to the shared provider page. */
export interface ProviderCredentialConfig {
  readonly fields: readonly ProviderCredentialField[];
  readonly prefill?: ((key: string, stored: string) => string) | undefined;
  readonly copyDescriptions?: Partial<Record<string, React.ReactNode>> | undefined;
  readonly copyHelp?: Partial<Record<string, ProviderConfigCopyHelp>> | undefined;
}

const GITHUB_FIELDS: readonly ProviderCredentialField[] = [
  {
    key: "GITHUB_CLIENT_ID",
    label: "Client ID",
    type: "text",
    required: true,
    description: (
      <>
        Register an OAuth application with GitHub to receive a client ID.{" "}
        <ConsoleLink href={GITHUB_OAUTH_CONSOLE_URL}>Open the GitHub application console</ConsoleLink>
      </>
    ),
  },
  {
    key: "GITHUB_CLIENT_SECRET",
    label: "Client secret",
    type: "password",
    required: true,
    description: (
      <>
        The matching secret from the same GitHub OAuth application.{" "}
        <ConsoleLink href={GITHUB_OAUTH_CONSOLE_URL}>Open the GitHub application console</ConsoleLink>
      </>
    ),
  },
  {
    key: "GITHUB_ORGANIZATION_ID",
    label: "Organization ID",
    type: "text",
    required: false,
    description: "Optional: restrict sign-in to members of one GitHub organization by its numeric ID.",
  },
];

const GITLAB_FIELDS: readonly ProviderCredentialField[] = [
  {
    key: "GITLAB_HOST",
    label: "Host",
    type: "text",
    required: true,
    placeholder: GITLAB_PUBLIC_HOST,
    description: "The public GitLab service, or the domain of a self-hosted GitLab installation.",
  },
  {
    key: "GITLAB_CLIENT_ID",
    label: "Application ID",
    type: "text",
    required: true,
    description: (
      <>
        Issued by your GitLab OAuth application.{" "}
        <ConsoleLink href={GITLAB_OAUTH_DOCS_URL}>Read the GitLab OAuth guide</ConsoleLink>
      </>
    ),
  },
  {
    key: "GITLAB_CLIENT_SECRET",
    label: "Secret",
    type: "password",
    required: true,
    description: (
      <>
        The matching secret from the same GitLab OAuth application.{" "}
        <ConsoleLink href={GITLAB_OAUTH_DOCS_URL}>Read the GitLab OAuth guide</ConsoleLink>
      </>
    ),
  },
];

const GITEA_FIELDS: readonly ProviderCredentialField[] = [
  {
    key: GITEA_HOST_KEY,
    label: "Host",
    type: "text",
    required: true,
    placeholder: GITEA_PUBLIC_HOST,
    description: "The address of the Gitea service members sign in with: the public service or a self-hosted one.",
  },
  {
    key: "GITEA_CLIENT_ID",
    label: "Client ID",
    type: "text",
    required: true,
    description: (
      <>
        Issued by your Gitea OAuth application.{" "}
        <ConsoleLink href={GITEA_PUBLIC_APPLICATIONS_URL}>Open the Gitea application settings</ConsoleLink>
      </>
    ),
  },
  {
    key: "GITEA_CLIENT_SECRET",
    label: "Client secret",
    type: "password",
    required: true,
    description: (
      <>
        The matching secret from the same Gitea OAuth application.{" "}
        <ConsoleLink href={GITEA_PUBLIC_APPLICATIONS_URL}>Open the Gitea application settings</ConsoleLink>
      </>
    ),
  },
];

const GOOGLE_FIELDS: readonly ProviderCredentialField[] = [
  {
    key: "GOOGLE_CLIENT_ID",
    label: "Client ID",
    type: "text",
    required: true,
    description: (
      <>
        Issued by your Google API project.{" "}
        <ConsoleLink href={GOOGLE_CLIENT_GUIDE_URL}>How to create OAuth credentials</ConsoleLink>
      </>
    ),
  },
  {
    key: "GOOGLE_CLIENT_SECRET",
    label: "Client secret",
    type: "password",
    required: true,
    description: (
      <>
        The matching secret from the same Google API project.{" "}
        <ConsoleLink href={GOOGLE_SECRET_GUIDE_URL}>Where to find the client secret</ConsoleLink>
      </>
    ),
  },
];

/** Credential configuration per provider route, keyed by route slug. */
export const PROVIDER_CREDENTIALS: Record<AuthProviderSlug, ProviderCredentialConfig> = {
  github: {
    fields: GITHUB_FIELDS,
    copyDescriptions: {
      "Origin URL": "Paste this address into the authorized-origin field of your GitHub OAuth application.",
      "Callback URI": "Paste this address into the callback field of your GitHub OAuth application.",
    },
    copyHelp: {
      "Origin URL": { href: GITHUB_OAUTH_CONSOLE_URL, label: "Open the GitHub application console" },
      "Callback URI": { href: GITHUB_OAUTH_CONSOLE_URL, label: "Open the GitHub application console" },
    },
  },
  gitlab: {
    fields: GITLAB_FIELDS,
    copyDescriptions: {
      "Callback URL": "Paste this address into the redirect URI field of your GitLab OAuth application.",
    },
    copyHelp: {
      "Callback URL": { href: GITLAB_OAUTH_DOCS_URL, label: "Read the GitLab OAuth guide" },
    },
  },
  gitea: {
    fields: GITEA_FIELDS,
    prefill: giteaHostPrefill,
    copyDescriptions: {
      "Callback URI": "Paste this address into the callback field of the OAuth application on your Gitea host.",
    },
    copyHelp: {
      "Callback URI": { href: giteaApplicationsHref, label: "Open this host's application settings" },
    },
  },
  google: {
    fields: GOOGLE_FIELDS,
    copyDescriptions: {
      "Origin URL": "Paste this address into the authorized JavaScript origins of your Google OAuth client.",
      "Callback URI": "Paste this address into the authorized redirect URIs of your Google OAuth client.",
    },
    copyHelp: {
      "Origin URL": { href: GOOGLE_OAUTH_CONSOLE_URL, label: "Open the Google credentials console" },
      "Callback URI": { href: GOOGLE_OAUTH_CONSOLE_URL, label: "Open the Google credentials console" },
    },
  },
};
