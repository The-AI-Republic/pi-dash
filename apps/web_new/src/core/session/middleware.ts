// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Session middleware (Data layer page). Any 401 from any request moves
// the session to the expired state (keeping the return URL); the router
// then sends the user to sign-in. Only 401 expires the session: 403 and
// other failures surface to their callers untouched.

import type { Middleware } from "@pidash/api-client";

import { useSessionStore } from "./store.js";

/** Expire the session when the response is a 401. */
export function sessionExpiredMiddleware(onUnauthorized: () => void): Middleware {
  return async (request, next) => {
    const response = await next(request);
    if (response.status === 401) {
      onUnauthorized();
    }
    return response;
  };
}

/** The middleware wired into the app client: 401s expire the store. */
export function createSessionMiddleware(): Middleware {
  return sessionExpiredMiddleware(() => {
    useSessionStore.getState().markExpired();
  });
}
