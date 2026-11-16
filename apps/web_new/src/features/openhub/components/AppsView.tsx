// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Apps page view (written from parity/specs/cloud.md §5, CLOUD-022 and
// CLOUD-030, and §6, CLOUD-026). The status read gates the catalog:
// only a connected status mounts it, and every other outcome explains
// itself — a stale session points at sign-out-and-back-in, a
// disconnected one shows the server's own detail, and a failed read
// (HTTP or transport alike) renders an error panel with a retry. A
// bounced connect outcome reports once and cleans its own address no
// matter which state the gate lands in.
import { Button, Spinner } from "@pidash/kit";
import * as React from "react";

import { notifyError } from "../../../shared/shell/index.js";
import { useOpenhubStatus } from "../api/queries.js";
import { cleanedBounceHref, hasBounceError, hasOutcomeParams } from "../lib/connect.js";
import { catalogFailureMessage } from "../lib/errors.js";
import { AppsCatalog } from "./AppsCatalog.js";

export const APPS_UNAVAILABLE_HEADING = "Apps are unavailable";
export const APPS_STALE_HEADING = "Reconnect to load your apps";

/** A connect that never started bounces back with an error parameter. */
export const CONNECTION_FAILED_TITLE = "Connection failed";
export const CONNECTION_FAILED_MESSAGE = "The connection did not start. Try again from the app card.";

/**
 * Stale-session guidance. The oracle pins the sign-out-and-back-in
 * wording (apps-states.spec.ts): a refresh alone cannot fix it.
 */
export const APPS_STALE_BODY =
  "Your session started before apps access was turned on, so the catalog cannot tell which apps are yours. " +
  "Sign out and back in to refresh your session.";

export const APPS_DISCONNECTED_FALLBACK = "The apps catalog is unavailable right now.";

function AppsFallback({ children }: { children: React.ReactNode }): React.ReactElement {
  return (
    <div className="flex flex-col gap-(--space-4)">
      <h1 className="text-h2 text-(--text)">Apps</h1>
      {children}
    </div>
  );
}

function StatusErrorPanel({ message, onRetry }: { message: string; onRetry: () => void }): React.ReactElement {
  return (
    <section aria-label={APPS_UNAVAILABLE_HEADING} className="flex flex-col items-start gap-(--space-3)">
      <h2 className="text-h3 text-(--text)">{APPS_UNAVAILABLE_HEADING}</h2>
      <p className="text-body text-(--text-muted)">{message}</p>
      <Button onClick={onRetry}>Retry</Button>
    </section>
  );
}

function StaleSessionPanel(): React.ReactElement {
  return (
    <section aria-label={APPS_STALE_HEADING} className="flex flex-col items-start gap-(--space-3)">
      <h2 className="text-h3 text-(--text)">{APPS_STALE_HEADING}</h2>
      <p className="text-body text-(--text-muted)">{APPS_STALE_BODY}</p>
    </section>
  );
}

function DisconnectedPanel({ detail }: { detail: string | undefined }): React.ReactElement {
  return (
    <section aria-label={APPS_UNAVAILABLE_HEADING} className="flex flex-col items-start gap-(--space-3)">
      <h2 className="text-h3 text-(--text)">{APPS_UNAVAILABLE_HEADING}</h2>
      <p className="text-body text-(--text-muted)">{detail ?? APPS_DISCONNECTED_FALLBACK}</p>
    </section>
  );
}

export function AppsView({ workspaceSlug }: { workspaceSlug: string }): React.ReactElement {
  const status = useOpenhubStatus(workspaceSlug);
  // Above the gate: the bounce reports no matter which state lands.
  // Reading the live address keeps the effect idempotent across
  // re-runs — after the cleanup there is nothing left to report, so
  // later searches stay silent (CLOUD-026).
  React.useEffect(() => {
    if (!hasOutcomeParams(window.location.search)) return;
    if (hasBounceError(window.location.search)) {
      notifyError(CONNECTION_FAILED_TITLE, CONNECTION_FAILED_MESSAGE);
    }
    window.history.replaceState(null, "", cleanedBounceHref(window.location));
  }, []);
  if (status.isPending) {
    return (
      <AppsFallback>
        <Spinner label="Loading apps" />
      </AppsFallback>
    );
  }
  if (status.isError) {
    return (
      <AppsFallback>
        <StatusErrorPanel message={catalogFailureMessage(status.error)} onRetry={() => void status.refetch()} />
      </AppsFallback>
    );
  }
  const answer = status.data;
  if (answer === undefined) {
    return (
      <AppsFallback>
        <Spinner label="Loading apps" />
      </AppsFallback>
    );
  }
  if (!answer.connected) {
    if (!answer.has_gateway_scopes) {
      return (
        <AppsFallback>
          <StaleSessionPanel />
        </AppsFallback>
      );
    }
    return (
      <AppsFallback>
        <DisconnectedPanel detail={answer.detail} />
      </AppsFallback>
    );
  }
  return <AppsCatalog workspaceSlug={workspaceSlug} />;
}
