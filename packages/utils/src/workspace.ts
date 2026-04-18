/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

// apple pi dash imports
import type { IWorkspace } from "@apple-pi-dash/types";

export const orderWorkspacesList = (workspaces: IWorkspace[]): IWorkspace[] =>
  workspaces.sort((a, b) => a.name.localeCompare(b.name));
