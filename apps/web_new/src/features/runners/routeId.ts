// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project runners route identity (NEWPIDASH-3). The project layout reads
// it on every project page to decide whether the page below the tab strip
// is a full-height area, so it lives apart from the runners feature's
// index and pulls nothing else into the project layout's chunk.

/** The file-route id of the project-scoped runners layout. */
export const PROJECT_RUNNERS_ROUTE_ID = "/$ws/_gated/projects/$projectId/runners";

/** True for the project runners layout route and every route beneath it. */
export function isProjectRunnersRouteId(routeId: string): boolean {
  return routeId === PROJECT_RUNNERS_ROUTE_ID || routeId.startsWith(`${PROJECT_RUNNERS_ROUTE_ID}/`);
}
