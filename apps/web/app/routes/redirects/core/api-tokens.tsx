/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { redirect } from "react-router";
import type { Route } from "./+types/api-tokens";

export const clientLoader = ({ params }: Route.ClientLoaderArgs) => {
  throw redirect(`/${params.workspaceSlug}/settings/account/api-tokens/`);
};

export default function ApiTokens() {
  return null;
}
