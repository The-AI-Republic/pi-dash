// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Desktop sign-in screen (AUTH-022, from parity/specs/auth.md). The bare
// screen behind the desktop entry routes: a failure alert above the card
// when a provider-side or aborted flow lands back with ?error=, then the
// edition's sign-in card (or the unavailable card where the server offers
// no hand-off). The screen never starts sign-in on its own: opening the
// system browser stays an explicit click.

import * as React from "react";

import { edition } from "../../../core/edition/index.js";
import { describeDesktopSignInFailure } from "../desktop.js";
import { DesktopUnavailableCard } from "./DesktopUnavailableCard.js";

export interface DesktopSignInScreenProps {
  /** Failure code carried back from a provider-side or aborted flow. */
  error?: string | undefined;
  /** Provider-supplied failure description, when one arrived. */
  errorDescription?: string | undefined;
}

export function DesktopSignInScreen({ error, errorDescription }: DesktopSignInScreenProps): React.ReactElement {
  const failure = describeDesktopSignInFailure({ error, errorDescription });
  const SignInCard = edition.slots?.desktopSignInCard ?? DesktopUnavailableCard;
  return (
    <main className="flex min-h-screen flex-col items-center justify-center gap-(--space-4) bg-(--bg) p-(--space-8)">
      {failure === null ? null : (
        <div
          role="alert"
          className="flex w-full max-w-90 flex-col gap-(--space-1) rounded-(--radius-control) border border-(--danger) p-(--space-4)"
        >
          <p className="text-emphasis text-(--text)">{failure.heading}</p>
          <p className="text-body text-(--text-muted)">{failure.body}</p>
        </div>
      )}
      <SignInCard />
    </main>
  );
}
