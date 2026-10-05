/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { isAxiosError } from "axios";
import { Button } from "@pi-dash/propel/button";
import { getNativeApi } from "@pi-dash/services";

/** The server the desktop app could not connect to, when `error` is a request
 * that never got an HTTP response. Undefined in browsers and for errors the
 * server itself returned, which keep the self-hosting maintenance message.
 */
export function unreachableDesktopServer(error: unknown): string | undefined {
  const native = getNativeApi();
  if (!native || !isAxiosError(error) || error.response) return undefined;
  return new URL(native.apiOrigin).host;
}

export function ServerUnreachableMessage({ server }: { server: string }) {
  return (
    <>
      <div className="flex flex-col gap-2.5">
        <h1 className="text-left text-18 font-semibold text-primary">Pi Dash could not reach {server}</h1>
        <span className="text-left text-14 font-medium text-secondary">
          The server did not respond. Check that you are connected to the network or VPN it is on, and that your system
          proxy settings allow connections to it, then try again.
        </span>
      </div>
      <div className="mt-1 flex items-center justify-start">
        <Button variant="primary" onClick={() => window.location.reload()}>
          Try again
        </Button>
      </div>
    </>
  );
}
