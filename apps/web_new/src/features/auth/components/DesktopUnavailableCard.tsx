// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Desktop unavailable-sign-in card (AUTH-023, from parity/specs/auth.md).
// Where the server offers no desktop sign-in hand-off, the desktop entry
// renders this explanatory card instead of a dead form, with an action
// that opens the web app in the system browser. The heading deliberately
// avoids the "Sign in to Pi Dash" wording an edition hand-off card is
// recognized by, so a misbuilt bundle fails its build check instead of
// passing silently.

import { Button } from "@pidash/kit";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { platform } from "../../../core/platform/index.js";
import { isWebAddress } from "../../../core/platform/urls.js";
import { bakedWebBaseUrl, fallbackToPlainNavigation, resolveBrowserAppUrl } from "../desktop.js";
import { authStepInputProps } from "../focus.js";

export function DesktopUnavailableCard(): React.ReactElement {
  const browserUrl = resolveBrowserAppUrl(bakedWebBaseUrl(), getAppClient().baseUrl);

  async function openWebApp(): Promise<void> {
    try {
      await platform.openExternal(browserUrl);
    } catch {
      if (isWebAddress(browserUrl)) fallbackToPlainNavigation(browserUrl);
    }
  }

  return (
    <section
      aria-label="Desktop sign-in unavailable"
      className="flex w-full max-w-90 flex-col items-center gap-(--space-4) text-center"
    >
      <div className="flex flex-col gap-(--space-1)">
        <h1 className="text-h2 text-(--text)">The desktop app can&apos;t sign you in yet</h1>
        <p className="text-body text-(--text-muted)">
          This server doesn&apos;t offer desktop sign-in yet. Use the web app in your browser for now.
        </p>
      </div>
      {browserUrl === "" ? null : (
        <Button variant="primary" size="large" {...authStepInputProps()} onClick={() => void openWebApp()}>
          Continue in the browser
        </Button>
      )}
    </section>
  );
}
