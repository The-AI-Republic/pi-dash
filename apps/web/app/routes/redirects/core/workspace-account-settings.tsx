/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { redirect } from "react-router";
import type { Route } from "./+types/workspace-account-settings";

// Bare `/:workspaceSlug/settings/account` (and anything deeper than a single tab
// segment) has no page of its own — send it to the default tab. Real tabs match
// the `:profileTabId` route, which out-ranks this splat.
export const clientLoader = ({ params, request }: Route.ClientLoaderArgs) => {
  const searchParams = new URL(request.url).searchParams.toString();
  throw redirect(`/${params.workspaceSlug}/settings/account/general${searchParams ? `?${searchParams}` : ""}`);
};

export default function WorkspaceAccountSettings() {
  return null;
}
