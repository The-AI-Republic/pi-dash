// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Retired sign-in address (AUTH-044). /sign-in forwards to the home route
// as a bare redirect: no email prefill, no return-path handling. The
// canonical sign-in entry is / (see routes/index.tsx). Desktop builds
// override this address with the desktop sign-in screen (AUTH-022).

import { createFileRoute, redirect } from "@tanstack/react-router";

/** Bare forward to the home route; any query params are dropped. */
export function redirectRetiredSignIn(): never {
  throw redirect({ to: "/" });
}

export const Route = createFileRoute("/sign-in")({
  beforeLoad: redirectRetiredSignIn,
});
