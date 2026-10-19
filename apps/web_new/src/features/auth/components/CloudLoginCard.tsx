// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud sign-in page (CLOUD-011-014, from parity/specs/cloud.md §3). The
// card offers one organizational-account action into the provider flow.
// A returning visitor with a pending target resumes that flow silently —
// with a replacing navigation, so the login entry leaves no history step —
// unless an error landed them here or this tab signed out explicitly. The
// edition fills the cloudLoginCard slot with this screen, so the OSS build
// keeps its bare /login redirect.

import { Button } from "@pidash/kit";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import {
  clearExplicitLogout,
  cloudHandoffTarget,
  cloudSignInUrl,
  CLOUD_SIGN_IN_HREF,
  parseCloudLoginSearch,
  readExplicitLogout,
  resolveCloudReturnTarget,
  shouldAttemptSilentSignIn,
} from "../cloudLogin.js";

const TERMS_HREF = "https://airepublic.com/terms";
const PRIVACY_HREF = "https://airepublic.com/privacy";
const HOME_HREF = "/home";
const RESUME_FAILURE = "We couldn't sign you in automatically. Continue below to try again.";

/** This tab's session storage, or null where the runtime exposes none. */
function tabStorage(): Pick<Storage, "getItem" | "setItem" | "removeItem"> | null {
  try {
    return globalThis.sessionStorage ?? null;
  } catch {
    return null;
  }
}

/** The page's own origin; the handoff's same-origin check depends on it. */
function browserOrigin(): string {
  return globalThis.window.location.origin;
}

interface Visit {
  /** Safe, loop-guarded target this visit carried; null when it carried none. */
  pendingTarget: string | null;
  /** Error parameter, if any. */
  error: string | null;
  /** True while the page is leaving for the provider without the card. */
  resuming: boolean;
}

/** Decide what this visit does, once, before the first paint. */
function readVisit(): Visit {
  const search = parseCloudLoginSearch(globalThis.window.location.search);
  const origin = browserOrigin();
  const safe = resolveCloudReturnTarget(search.nextPath, origin);
  const attempt = shouldAttemptSilentSignIn({
    pendingTarget: safe,
    error: search.error,
    explicitlyLoggedOut: readExplicitLogout(tabStorage()),
  });
  return { pendingTarget: safe, error: search.error, resuming: attempt };
}

export interface CloudLoginCardProps {
  /** API base the handoff lives on; defaults to the app client's. */
  apiBase?: string | undefined;
}

export function CloudLoginCard({ apiBase }: CloudLoginCardProps = {}): React.ReactElement {
  const [visit] = React.useState<Visit>(readVisit);

  React.useEffect(() => {
    if (!visit.resuming || visit.pendingTarget === null) return;
    const base = apiBase ?? getAppClient().baseUrl;
    const origin = browserOrigin();
    globalThis.window.location.replace(cloudSignInUrl(base, origin, cloudHandoffTarget(visit.pendingTarget, origin)));
  }, [apiBase, visit]);

  function startManualSignIn(event: React.MouseEvent<HTMLAnchorElement>): void {
    event.preventDefault();
    // A manual attempt re-arms silent sign-in for later visits.
    clearExplicitLogout(tabStorage());
    const base = apiBase ?? getAppClient().baseUrl;
    const origin = browserOrigin();
    const raw = new URLSearchParams(globalThis.window.location.search).get("next_path");
    globalThis.window.location.assign(cloudSignInUrl(base, origin, cloudHandoffTarget(raw, origin)));
  }

  if (visit.resuming) return <></>;

  return (
    <main className="flex min-h-screen items-center justify-center bg-(--bg) p-(--space-8)">
      <div className="flex w-full max-w-4xl flex-col-reverse items-center gap-(--space-8) lg:flex-row lg:justify-center lg:gap-(--space-12)">
        <section className="flex w-full max-w-lg flex-col gap-(--space-3)">
          <p className="text-meta font-semibold text-(--accent)">Pi Dash: Your AI Workers Management Platform</p>
          <h1 className="text-h2 text-(--text)">Manage Your Own AI Like A Boss</h1>
          <a href={HOME_HREF} className="text-body w-fit text-(--text-muted) underline">
            Explore Pi Dash
          </a>
        </section>
        <section
          aria-label="Cloud sign-in"
          className="flex w-full max-w-md flex-col items-center gap-(--space-6) rounded-(--radius-card) border border-(--border) bg-(--bg) p-(--space-8)"
        >
          <div className="flex flex-col items-center gap-(--space-2) text-center">
            <h2 className="text-h2 text-(--text)">Sign in to Pi Dash</h2>
            <p className="text-body text-(--text-muted)">Continue with your AI Republic account.</p>
            {visit.error === null ? null : (
              <p role="alert" className="text-body text-(--danger)">
                {RESUME_FAILURE}
              </p>
            )}
          </div>
          <a href={CLOUD_SIGN_IN_HREF} onClick={startManualSignIn} className="w-full">
            <Button variant="primary" size="large" className="w-full">
              Continue with AI Republic
            </Button>
          </a>
          <p className="text-meta text-center text-(--text-muted)">
            By signing in, you agree to our{" "}
            <a href={TERMS_HREF} target="_blank" rel="noopener noreferrer" className="underline">
              Terms
            </a>{" "}
            and{" "}
            <a href={PRIVACY_HREF} target="_blank" rel="noopener noreferrer" className="underline">
              Privacy Policy
            </a>
            .
          </p>
        </section>
      </div>
    </main>
  );
}
