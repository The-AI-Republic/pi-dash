// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
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
