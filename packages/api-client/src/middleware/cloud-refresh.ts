// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud session rotation (CLOUD-015, from parity/specs/cloud.md §4). A 401
// first rotates the cookie pair through POST /api/auth/refresh/ and replays
// the original request once; concurrent 401s share one in-flight rotation,
// each request replays at most once, and the rotation call itself is never
// replayed. Only a rejected rotation or replay (a 401/403 *response*)
// passes the 401 on to the outer expiry layer for the sign-in bounce;
// transport failures, timeouts and other statuses propagate untouched so
// the page stays put. The cloud edition lists this in `api`; the OSS
// edition lists nothing and keeps its expire-on-401 behavior.
import type { Middleware, MiddlewareContext, TransportResponse } from "../client.js";
import { ApiError } from "../errors.js";

/** The rotation path, derived against the request's own origin. */
export const CLOUD_REFRESH_PATH = "/api/auth/refresh/";

/** True when the request IS the rotation call (it is never rotated or replayed itself). */
export function isRefreshCall(url: string): boolean {
  try {
    return new URL(url).pathname === CLOUD_REFRESH_PATH;
  } catch {
    return false;
  }
}

type RotationOutcome = "rotated" | "rejected" | "soft";

/**
 * Share one rotation across the 401s that arrive while it is in flight.
 * A rejected rotation (401/403 response) means the session is dead; any
 * other non-success status is soft (it says nothing about the session);
 * a thrown rotation (transport, timeout, abort) rejects the shared promise
 * so every waiter sees the same transient failure.
 */
export function createCloudRefreshMiddleware(): Middleware {
  let inflight: Promise<RotationOutcome> | null = null;

  const rotate = (
    refresh: MiddlewareContext,
    next: (request: MiddlewareContext) => Promise<TransportResponse>
  ): Promise<RotationOutcome> => {
    if (inflight === null) {
      inflight = (async (): Promise<RotationOutcome> => {
        try {
          const answer = await next(refresh);
          if (answer.status >= 200 && answer.status < 300) return "rotated";
          if (answer.status === 401 || answer.status === 403) return "rejected";
          return "soft";
        } finally {
          inflight = null;
        }
      })();
    }
    return inflight;
  };

  return async (request, next) => {
    const answer = await next(request);
    if (answer.status !== 401 || isRefreshCall(request.url)) return answer;
    let rotation: string;
    try {
      rotation = new URL(CLOUD_REFRESH_PATH, request.url).toString();
    } catch {
      // No origin to rotate against: leave the verdict to the outer layers.
      return answer;
    }
    const outcome = await rotate({ method: "POST", url: rotation, headers: {} }, next);
    if (outcome === "rotated") return next(request);
    if (outcome === "rejected") return answer;
    throw new ApiError({
      status: 0,
      code: "network",
      message: "The session rotation failed without answering; retry with the current session.",
    });
  };
}
