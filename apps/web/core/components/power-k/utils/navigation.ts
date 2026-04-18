/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

// apple pi dash imports
import { joinUrlPath } from "@apple-pi-dash/utils";
// local imports
import type { TPowerKContext } from "../core/types";

export const handlePowerKNavigate = (context: TPowerKContext, routerSegments: (string | undefined)[]) => {
  const validRouterSegments = routerSegments.filter((segment) => segment !== undefined);

  if (validRouterSegments.length === 0) {
    console.warn("No valid router segments provided", routerSegments);
    return;
  }

  if (validRouterSegments.length !== routerSegments.length) {
    console.warn("Some of the router segments are undefined", routerSegments);
  }

  const route = joinUrlPath(...validRouterSegments);
  context.router.push(route);
};
