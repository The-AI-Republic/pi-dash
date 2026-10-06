/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { getNativeApi } from "./desktop-api-adapter";

// The one place the page refreshes a session from. An edition whose session
// can be refreshed registers how (from `./ee/init`); every interceptor that
// wants a refresh then shares a single in-flight attempt instead of keeping
// its own, where two of them could rotate the session at the same time.

/** `baseURL` is the API base of the instance whose request got the 401. */
export type SessionRefresher = (baseURL: string) => Promise<void>;

let refresher: SessionRefresher | undefined;
let inFlight: Promise<void> | undefined;

export function registerSessionRefresher(refresh: SessionRefresher): void {
  refresher = refresh;
}

/** Refresh the session, joining an attempt already in flight.
 *
 * Returns undefined when the page has nothing to refresh with, which makes
 * the 401 that prompted the call final: the edition has no refresh (OSS), or
 * the desktop's native transport already refreshed and replayed the request
 * before handing over this response.
 */
export function refreshSession(baseURL: string): Promise<void> | undefined {
  if (!refresher || getNativeApi()?.refreshesSession) return undefined;
  inFlight ??= refresher(baseURL).finally(() => {
    inFlight = undefined;
  });
  return inFlight;
}
