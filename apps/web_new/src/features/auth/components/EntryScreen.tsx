// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The web entry card's page frame (AUTH-001–011, parity/specs/auth.md):
// the community sign-in card centered on the app canvas. Both the root
// entry and a cloud build's /login before its own card lands render this
// frame, so it lives in the auth feature rather than in a route module.
import * as React from "react";

import { SignInCard } from "./SignInCard.js";

export interface EntryScreenProps {
  next?: string | undefined;
  unsafeNext?: string | undefined;
  email?: string | undefined;
  errorCode?: string | undefined;
  invitationId?: string | undefined;
  invitationSlug?: string | undefined;
}

export function EntryScreen({
  next,
  unsafeNext,
  email,
  errorCode,
  invitationId,
  invitationSlug,
}: EntryScreenProps): React.ReactElement {
  return (
    <main className="flex min-h-screen items-center justify-center bg-(--bg) p-(--space-8)">
      <SignInCard
        next={next}
        unsafeNext={unsafeNext}
        initialEmail={email ?? ""}
        initialErrorCode={errorCode ?? null}
        invitationId={invitationId}
        invitationSlug={invitationSlug}
      />
    </main>
  );
}
