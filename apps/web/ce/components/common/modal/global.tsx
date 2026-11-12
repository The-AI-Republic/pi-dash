/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";

type TGlobalModalsProps = {
  workspaceSlug: string;
};

/**
 * GlobalModals component manages all workspace-level modals across Pi Dash applications.
 *
 * No modals are mounted here in the community edition at the moment; this stays
 * as the extension point the enterprise overlay builds on.
 */
export const GlobalModals = observer(function GlobalModals(_props: TGlobalModalsProps) {
  return null;
});
