/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { Navigate } from "react-router";

/**
 * Redirect /schedulers → /schedulers/calendar so the default landing for
 * the Schedulers nav entry is the calendar view. The list view stays
 * reachable at /schedulers/list via the in-page tab bar.
 */
export default function ProjectSchedulersIndexPage() {
  return <Navigate to="calendar" replace />;
}
