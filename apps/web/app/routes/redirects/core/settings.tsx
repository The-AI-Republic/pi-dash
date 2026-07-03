/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { redirect } from "react-router";
import type { Route } from "./+types/settings";

// `/settings` (and `/settings/profile` with no tab) has no page of its own, so
// hitting it directly used to fall through to the 404 handler. Redirect it to
// the default profile settings tab instead. The more specific
// `settings/profile/:profileTabId` route out-ranks this splat, so real tabs are
// unaffected.
export const clientLoader = ({ request }: Route.ClientLoaderArgs) => {
  const searchParams = new URL(request.url).searchParams.toString();
  throw redirect(`/settings/profile/general${searchParams ? `?${searchParams}` : ""}`);
};

export default function SettingsIndexRedirect() {
  return null;
}
