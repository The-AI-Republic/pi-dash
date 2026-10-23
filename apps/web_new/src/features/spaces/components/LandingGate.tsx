// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public landing gate (SPC-005, SPC-006). Resolves the visitor's session
// behind a branded loader, then: anonymous visitors get the spaces auth
// card (SPC-058–064) carrying the validated return path, any prefill and
// any coded failure; signed-in visitors with a valid `next_path` are
// replace-navigated to it; signed-in visitors without one get the
// paste-a-link guidance view. Dropped return addresses land on the
// guidance view, and a return address naming the landing itself never
// navigates (it would loop). The account control and the product badge
// are slice D's (SPC-012, SPC-014) and arrive through the documented
// slots; the gate renders fine without them.
//
// Gate DOM contract (for the area gate's new-app driver; stable by test):
// - main[data-testid="space-landing"] in the loader, auth and guidance
//   states (the redirect state renders nothing but the navigation).
// - Auth state mounts section[data-testid="space-auth-card"] (slice C).
// - Guidance state mounts [data-testid="space-landing-guidance"] with an
//   h1 and the paste-a-link line.

import type { InstanceBootstrap } from "@pidash/api-client";
import { Spinner } from "@pidash/kit";
import { useQuery } from "@tanstack/react-query";
import { Navigate } from "@tanstack/react-router";
import { Link2 } from "lucide-react";
import * as React from "react";

import { useDocumentHead } from "../../../shared/shell/document.js";
import { spaceRuntime } from "../api/client.js";
import { spaceInstanceQueryOptions, spaceViewerQueryOptions } from "../api/queries.js";
import { SpaceAuthCard, type SpaceAuthInstanceCapabilities } from "../auth/SpaceAuthCard.js";
import { SPACE_HEAD_ROBOTS } from "../lib/boardHead.js";
import {
  isSpaceLandingTarget,
  resolveSpaceReturnTarget,
  validateSpaceReturnPath,
  type SpaceLandingSearch,
} from "../lib/returnPath.js";

/** Product mark served locally, shared with the startup screens' asset. */
export const SPACE_PRODUCT_MARK_SRC = "/icon.svg";

/** Product name for the gate's brand surfaces. */
export const SPACE_PRODUCT_NAME = "Pi Dash";

/** Live-document title for the landing states. */
export const SPACE_LANDING_TITLE = "Pi Dash — Public boards";

/** Guidance heading: the visitor is signed in with nowhere to go yet. */
export const SPACE_GUIDANCE_HEADING = "You are signed in";

/** Guidance line: how a signed-in visitor reaches a board. */
export const SPACE_GUIDANCE_LINE = "To open a public board, paste its link into the address bar.";

/**
 * Instance flags the auth card routes on, read off the settled public
 * bootstrap. A missing bootstrap disables every method; the boot gate
 * normally keeps that state from rendering.
 */
export function spaceLandingCapabilities(
  bootstrap: InstanceBootstrap | null | undefined
): SpaceAuthInstanceCapabilities {
  const config = bootstrap?.config;
  return {
    smtpConfigured: config?.is_smtp_configured ?? false,
    magicLoginEnabled: config?.is_magic_login_enabled ?? false,
    emailPasswordEnabled: config?.is_email_password_enabled ?? false,
    googleEnabled: config?.is_google_enabled ?? false,
    githubEnabled: config?.is_github_enabled ?? false,
    gitlabEnabled: config?.is_gitlab_enabled ?? false,
    giteaEnabled: config?.is_gitea_enabled ?? false,
  };
}

export interface LandingGateProps {
  /** Settled landing search (`next_path`, `email`, `error_code`). */
  search: SpaceLandingSearch;
  /** Slice D (SPC-014): the guidance view's account control. */
  account?: React.ReactNode;
  /** Slice D (SPC-012): the product badge for the auth and guidance views. */
  badge?: React.ReactNode;
}

function validatedNextPath(search: SpaceLandingSearch): string | undefined {
  if (search.nextPath === undefined) return undefined;
  return validateSpaceReturnPath(search.nextPath) ?? undefined;
}

function LandingLoader(): React.ReactElement {
  return (
    <main
      data-testid="space-landing"
      role="status"
      aria-label="Loading public boards"
      className="flex min-h-screen flex-col items-center justify-center gap-(--space-5) bg-(--bg)"
    >
      <img src={SPACE_PRODUCT_MARK_SRC} alt={SPACE_PRODUCT_NAME} width={48} height={48} />
      <Spinner size="large" label="Loading public boards" />
    </main>
  );
}

export interface LandingGuidanceProps {
  account?: React.ReactNode;
  badge?: React.ReactNode;
}

export function LandingGuidance({ account, badge }: LandingGuidanceProps): React.ReactElement {
  return (
    <main data-testid="space-landing" className="flex min-h-screen flex-col bg-(--bg) text-(--text)">
      <div className="flex items-center justify-between px-(--space-8) pt-(--space-6)">
        <img src={SPACE_PRODUCT_MARK_SRC} alt={SPACE_PRODUCT_NAME} width={32} height={32} />
        {account}
      </div>
      <div
        data-testid="space-landing-guidance"
        className="flex flex-1 flex-col items-center justify-center gap-(--space-4) px-(--space-8) pb-(--space-16) text-center"
      >
        <span
          aria-hidden="true"
          className="flex h-(--space-16) w-(--space-16) items-center justify-center rounded-(--radius-full) bg-(--accent-subtle) text-(--accent)"
        >
          <Link2 size={28} strokeWidth={2} />
        </span>
        <h1 className="text-h2">{SPACE_GUIDANCE_HEADING}</h1>
        <p className="text-body max-w-md text-(--text-muted)">{SPACE_GUIDANCE_LINE}</p>
      </div>
      {badge}
    </main>
  );
}

export function LandingGate({ search, account, badge }: LandingGateProps): React.ReactElement {
  useDocumentHead({ title: SPACE_LANDING_TITLE, robots: SPACE_HEAD_ROBOTS });
  const viewer = useQuery(spaceViewerQueryOptions(spaceRuntime.client));
  const instance = useQuery(spaceInstanceQueryOptions(spaceRuntime.client));
  if (viewer.isPending || instance.isPending) return <LandingLoader />;
  // A settled viewer is either the member or null (anonymous); anything
  // else would be a transient TanStack state that resolves on its own.
  if (viewer.data === null) {
    return (
      <main
        data-testid="space-landing"
        className="flex min-h-screen flex-col items-center justify-center gap-(--space-6) bg-(--bg) px-(--space-4) py-(--space-8)"
      >
        <SpaceAuthCard
          instance={spaceLandingCapabilities(instance.data?.bootstrap)}
          nextPath={validatedNextPath(search)}
          initialEmail={search.email}
          initialErrorCode={search.errorCode}
        />
        {badge}
      </main>
    );
  }
  const nextPath = validatedNextPath(search);
  if (nextPath !== undefined) {
    const target = resolveSpaceReturnTarget(nextPath);
    if (!isSpaceLandingTarget(target)) {
      return <Navigate to={target.to} search={target.search} replace />;
    }
  }
  return <LandingGuidance account={account} badge={badge} />;
}
