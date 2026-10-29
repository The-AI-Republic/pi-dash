// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Endpoint-traffic classification (NEWFRONT-772, EXP-024). Scenarios that
// capture request traffic must count endpoint calls only: a substring
// match on "/api/" mistakes dev-server module fetches such as
// /src/.../api/*.ts for endpoint traffic, while the oracle's production
// build emits no such paths.

/** True when the request URL is an endpoint call under /api/. */
export function isParityApiRequestPath(url: string): boolean {
  let pathname: string;
  try {
    pathname = new URL(url).pathname;
  } catch {
    // Unparseable URLs cannot be endpoint calls; ignore them.
    return false;
  }
  return pathname.startsWith("/api/");
}
