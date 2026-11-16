// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Legacy sign-up address (AUTH-011). /accounts/sign-up forwards to the
// canonical card at /sign-up/ after hydration. A full-document navigation
// keeps the trailing slash the old app serves; a router redirect would
// strip it.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

/** Canonical sign-up address, trailing slash included. */
export const LEGACY_SIGN_UP_TARGET = "/sign-up/";

export function LegacySignUpRedirect(): React.ReactElement | null {
  React.useEffect(() => {
    globalThis.window.location.replace(LEGACY_SIGN_UP_TARGET);
  }, []);
  return null;
}

export const Route = createFileRoute("/accounts/sign-up")({
  component: LegacySignUpRedirect,
});
