// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Auth-method card (ADM-024, ADM-026, from parity/specs/admin.md
// §4.2/§4.4): the registry row with its icon, pinned name and
// description, and the control branch for its kind — toggle-only for
// magic-link and password cards, a configure link until an OAuth
// card's credentials land, then an edit link plus the enable toggle.
// The GitHub mark swaps with the resolved console theme.
import { resolveTheme, Switch } from "@pidash/kit";
import { GitBranch, Globe, Lock, Mail, Server } from "lucide-react";
import * as React from "react";

import { GOD_MODE_MOUNT } from "../adminShell.js";
import type { AuthMethodDef } from "../authMethods.js";
import { useAdminThemeStore } from "../theme.js";

const GITHUB_LIGHT_SRC = "/logos/github-black.png";
const GITHUB_DARK_SRC = "/logos/github-white.png";

function MethodGlyph({ methodKey }: { methodKey: string }): React.ReactElement {
  switch (methodKey) {
    case "unique-codes":
      return <Mail size={20} aria-hidden="true" />;
    case "passwords":
      return <Lock size={20} aria-hidden="true" />;
    case "google":
      return <Globe size={20} aria-hidden="true" />;
    case "gitlab":
      return <GitBranch size={20} aria-hidden="true" />;
    case "gitea":
      return <Server size={20} aria-hidden="true" />;
    default:
      return <Globe size={20} aria-hidden="true" />;
  }
}

export interface AdminAuthMethodCardProps {
  method: AuthMethodDef;
  /** Stored on/off for the method enable flag. */
  enabled: boolean;
  /** Whether the OAuth credentials are present (toggle-only cards: true). */
  configured: boolean;
  /** Whole-group lock while any configuration save is in flight. */
  saving: boolean;
  /** Flip the enable flag; the caller guards and persists. */
  onToggle: (next: boolean) => void;
}

export function AdminAuthMethodCard({
  method,
  enabled,
  configured,
  saving,
  onToggle,
}: AdminAuthMethodCardProps): React.ReactElement {
  const themeMode = useAdminThemeStore((state) => state.mode);
  const labelId = `admin-auth-name-${method.key}`;
  const toggleId = `admin-auth-toggle-${method.key}`;
  const providerHref =
    method.providerSlug === undefined ? null : `${GOD_MODE_MOUNT}/authentication/${method.providerSlug}/`;
  const showToggle = method.control === "toggle" || configured;
  return (
    <div
      data-testid={`admin-auth-card-${method.key}`}
      className="flex w-full items-center gap-(--space-4) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-4) py-(--space-3)"
    >
      <div className="flex min-w-0 flex-1 items-center gap-(--space-3)">
        <span
          aria-hidden="true"
          className="flex size-10 shrink-0 items-center justify-center rounded-full bg-(--subtle) text-(--text-muted)"
        >
          {method.key === "github" ? (
            <img
              src={resolveTheme(themeMode) === "dark" ? GITHUB_DARK_SRC : GITHUB_LIGHT_SRC}
              alt=""
              width={20}
              height={20}
              className="size-5"
            />
          ) : (
            <MethodGlyph methodKey={method.key} />
          )}
        </span>
        <span className="min-w-0 flex-1">
          <span id={labelId} className="text-emphasis block truncate font-medium text-(--text)">
            {method.name}
          </span>
          <span className="text-body block text-(--text-muted)">{method.description}</span>
        </span>
      </div>
      <div className="flex shrink-0 items-center gap-(--space-3)">
        {method.control === "oauth" && providerHref !== null ? (
          <a
            data-testid={`admin-auth-link-${method.key}`}
            href={providerHref}
            className="text-body font-medium text-(--accent) hover:underline"
          >
            {configured ? "Edit" : "Configure"}
          </a>
        ) : null}
        {showToggle ? (
          <Switch
            checked={enabled}
            disabled={saving}
            onCheckedChange={onToggle}
            aria-labelledby={labelId}
            data-testid={toggleId}
          />
        ) : null}
      </div>
    </div>
  );
}
