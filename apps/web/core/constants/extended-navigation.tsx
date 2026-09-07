/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { ReactNode } from "react";
// NB: `@pi-dash/types` exports a same-named but different interface; this must
// be the `@pi-dash/constants` one, which is what `SidebarItem` accepts.
import type { IWorkspaceSidebarNavigationItem } from "@pi-dash/constants";

/**
 * A contributed sidebar item, optionally carrying its own icon.
 *
 * Built-in rows resolve theirs from a key lookup in
 * `ce/components/workspace/sidebar/helper`, which cannot know about a key a
 * downstream build introduces. Letting the item supply an icon avoids copying
 * that map into an overlay, where it would drift from upstream.
 */
export type TExtendedNavigationItem = IWorkspaceSidebarNavigationItem & {
  icon?: ReactNode;
};

/**
 * Workspace sidebar items contributed by the running build.
 *
 * Mirrors `app/routes/extended.ts`: open source exports an empty array and a
 * downstream build replaces this whole file to add its own rows. Kept as a
 * plain module rather than a registry so there is nothing to register with and
 * no ordering to reason about — the array is the contract.
 *
 * Items render after the workspace-pinned links (Projects, Runners, Prompts,
 * Schedulers), in declaration order.
 */
export const extendedNavigationItems: TExtendedNavigationItem[] = [];
