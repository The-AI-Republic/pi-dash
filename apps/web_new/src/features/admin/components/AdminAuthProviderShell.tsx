// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Provider sub-page shell (ADM-027, from parity/specs/admin.md §4.5):
// the borderless method card with the provider identity and the master
// enable toggle, over the credential form once configuration loads or
// a five-bar skeleton before it. The tab title is provider-specific.
// Identity strings are pinned by the parity scenario.
import { resolveTheme, Skeleton, Switch } from "@pidash/kit";
import { GitBranch, Globe, Server } from "lucide-react";
import * as React from "react";

import { useDocumentHead } from "../../../shared/shell/index.js";
import { useAdminThemeStore } from "../theme.js";
import { AUTH_PROVIDER_SKELETON_BARS, type AuthProviderDef } from "../providerAuth.js";

const GITHUB_LIGHT_SRC = "/logos/github-black.png";
const GITHUB_DARK_SRC = "/logos/github-white.png";

function ProviderGlyph({ slug }: { slug: string }): React.ReactElement {
  switch (slug) {
    case "gitlab":
      return <GitBranch size={20} aria-hidden="true" />;
    case "gitea":
      return <Server size={20} aria-hidden="true" />;
    default:
      return <Globe size={20} aria-hidden="true" />;
  }
}

export interface AdminAuthProviderShellProps {
  provider: AuthProviderDef;
  /** Stored on/off for the provider enable flag. */
  enabled: boolean;
  /** Locked while the master save flies or before configuration loads. */
  disabled: boolean;
  /** Flip the enable flag; the caller persists without a last-method guard. */
  onToggle: (next: boolean) => void;
  /** Configuration loaded: the body shows the form, else the skeleton. */
  ready: boolean;
  children: React.ReactNode;
}

export function AdminAuthProviderShell({
  provider,
  enabled,
  disabled,
  onToggle,
  ready,
  children,
}: AdminAuthProviderShellProps): React.ReactElement {
  useDocumentHead({ title: provider.title, robots: "noindex, nofollow" });
  const themeMode = useAdminThemeStore((state) => state.mode);
  const labelId = `admin-auth-provider-name-${provider.slug}`;
  return (
    <div
      data-testid="admin-auth-provider"
      className="mx-auto flex w-full max-w-3xl flex-col gap-(--space-6) p-(--space-6)"
    >
      <div
        data-testid="admin-auth-provider-card"
        className="flex w-full items-center gap-(--space-4) px-(--space-1) py-(--space-2)"
      >
        <span
          aria-hidden="true"
          className="flex size-11 shrink-0 items-center justify-center rounded-(--radius-control) bg-(--subtle) text-(--text-muted)"
        >
          {provider.slug === "github" ? (
            <img
              src={resolveTheme(themeMode) === "dark" ? GITHUB_DARK_SRC : GITHUB_LIGHT_SRC}
              alt=""
              width={22}
              height={22}
              className="size-[22px]"
            />
          ) : (
            <ProviderGlyph slug={provider.slug} />
          )}
        </span>
        <span className="min-w-0 flex-1">
          <span id={labelId} data-testid="admin-auth-provider-name" className="text-h3 block truncate text-(--text)">
            {provider.name}
          </span>
          <span data-testid="admin-auth-provider-description" className="text-body block text-(--text-muted)">
            {provider.description}
          </span>
        </span>
        <Switch
          checked={enabled}
          disabled={disabled}
          onCheckedChange={onToggle}
          aria-labelledby={labelId}
          data-testid="admin-auth-provider-toggle"
        />
      </div>
      {ready ? (
        children
      ) : (
        <div
          data-testid="admin-auth-provider-skeleton"
          role="status"
          aria-label={`Loading ${provider.name} authentication settings`}
          className="flex flex-col gap-(--space-3)"
        >
          {Array.from({ length: AUTH_PROVIDER_SKELETON_BARS }, (_, index) => (
            <Skeleton key={index} height="50px" width={index === 0 ? "30%" : index === 4 ? "55%" : "100%"} />
          ))}
        </div>
      )}
    </div>
  );
}
