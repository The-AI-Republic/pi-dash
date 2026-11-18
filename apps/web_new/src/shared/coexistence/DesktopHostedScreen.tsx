// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Desktop hand-off surface (M-01/NEWFRONT-92). When the app window lands on
// an address another frontend owns, this stands in for that screen: it
// opens the same address on the hosted web app in the system browser and
// then explains what happened, instead of leaving the window on a page
// that says the address does not exist when it does.
//
// It also copes with the window having no way out — no native bridge, a
// refusal from the operating system, or a build with no hosted address
// baked in. Each of those says so plainly and, where there is an address
// to try again, offers another attempt. Only the desktop build reaches
// this component; on the web the browser already is the hosted app.

import { Button, Spinner } from "@pidash/kit";
import * as React from "react";

import { platform } from "../../core/platform/index.js";
import { bakedHostedOrigin } from "../../core/platform/shareUrl.js";
import { useDocumentHead } from "../shell/document.js";
import { currentAddress, hostedAddressFor } from "./desktopHandOff.js";

export const HOSTED_SCREEN_TITLE = "Opening this screen on the web";
export const HOSTED_SCREEN_HEADING = "This screen opens in your browser";
export const HOSTED_SCREEN_MOVING = "Handing it to your browser now. This window stays on the desktop app.";
export const HOSTED_SCREEN_OPENED =
  "Your browser is opening it now. Continue there; this window stays on the desktop app.";
export const HOSTED_SCREEN_FAILURE_HEADING = "Could not open your browser";
export const HOSTED_SCREEN_RETRY = "Try again";
export const HOSTED_SCREEN_NO_ADDRESS =
  "This build has no web app address configured, so this screen cannot be opened from the desktop app.";
export const HOSTED_SCREEN_REFUSED =
  "Your browser did not accept the address. Try again, or open the web app directly.";

type HandOffState =
  | { kind: "opening" }
  | { kind: "opened" }
  | { kind: "failed"; address: string | null; reason: string };

/**
 * Where the browser goes for a handed-off address; null when none is
 * configured. The resolver only ever produces an http(s) address, so the
 * only failure worth handling is a build without one.
 */
export function hostedScreenAddressFor(path: string): string | null {
  try {
    return hostedAddressFor(path, bakedHostedOrigin());
  } catch {
    return null;
  }
}

export function DesktopHostedScreen(): React.ReactElement {
  useDocumentHead({ title: HOSTED_SCREEN_TITLE, robots: "noindex, nofollow", fallback: true });
  const address = currentAddress();

  const [state, setState] = React.useState<HandOffState>({ kind: "opening" });
  const [attempt, setAttempt] = React.useState(0);
  // One open per address and attempt: a re-render (or React's double-invoked
  // effect in development) must not hand the same address to the browser
  // twice, while a new address under the still-mounted surface does open.
  const openedFor = React.useRef<string | null>(null);

  React.useEffect(() => {
    if (openedFor.current === address) return;
    openedFor.current = address;
    const target = hostedScreenAddressFor(address);
    if (target === null) {
      setState({ kind: "failed", address: null, reason: HOSTED_SCREEN_NO_ADDRESS });
      return;
    }
    setState({ kind: "opening" });
    void platform.openExternal(target).then(
      () => setState({ kind: "opened" }),
      (error: unknown) => setState({ kind: "failed", address: target, reason: reasonOf(error) })
    );
  }, [address, attempt]);

  const retry = (): void => {
    openedFor.current = null;
    setAttempt((n) => n + 1);
  };

  if (state.kind === "failed") {
    return (
      <main className="flex min-h-screen flex-col items-center justify-center gap-(--space-4) bg-(--bg) p-(--space-8) text-center">
        <h1 className="text-h2 text-(--text)">{HOSTED_SCREEN_FAILURE_HEADING}</h1>
        <p role="alert" className="text-body max-w-md text-(--text-muted)">
          {state.reason}
        </p>
        {state.address === null ? null : (
          <>
            <p className="text-meta max-w-md break-all text-(--text-muted)">{state.address}</p>
            <Button variant="primary" onClick={retry}>
              {HOSTED_SCREEN_RETRY}
            </Button>
          </>
        )}
      </main>
    );
  }

  return (
    <main className="flex min-h-screen flex-col items-center justify-center gap-(--space-4) bg-(--bg) p-(--space-8) text-center">
      <Spinner size="large" label={HOSTED_SCREEN_HEADING} />
      <h1 className="text-h2 text-(--text)">{HOSTED_SCREEN_HEADING}</h1>
      <p className="text-body max-w-md text-(--text-muted)">
        {state.kind === "opened" ? HOSTED_SCREEN_OPENED : HOSTED_SCREEN_MOVING}
      </p>
    </main>
  );
}

function reasonOf(error: unknown): string {
  return error instanceof Error && error.message !== "" ? error.message : HOSTED_SCREEN_REFUSED;
}
