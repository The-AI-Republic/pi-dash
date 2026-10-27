// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud slot values for the views area (parity/specs/views.md §14,
// VIEW-019/020/040; Decisions D23–D25). The private cloud edition module
// references these through the @web_new alias; the OSS edition leaves
// every slot absent. Each write goes through the standard view PATCH
// with its own key and only counts when the echo carries the value
// back — the server stays the authority, and a build whose server drops
// the key fails loudly instead of faking the state.

import {
  publishProjectView,
  setProjectViewAccess,
  setWorkspaceViewAccess,
  unpublishProjectView,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import type {
  ViewAccessSlot,
  ViewAccessStoreInput,
  ViewPublishInput,
  ViewPublishResult,
  ViewPublishSlot,
  WorkspaceViewLayoutsSlot,
} from "../../../core/edition/index.js";
import { platform } from "../../../core/platform/index.js";
import { isViewLive } from "../lib/projectViews.js";
import { CloudViewAccessField } from "./CloudViewAccessField.js";

/** The access choice the server refused to store. */
export const VIEW_ACCESS_STORE_FAILED = "The server did not store the access choice.";
/** The publish the server refused to honor. */
export const VIEW_PUBLISH_FAILED = "The server did not publish the view.";
/** The unpublish the server refused to honor. */
export const VIEW_UNPUBLISH_FAILED = "The server did not unpublish the view.";

/**
 * Layouts the cloud edition offers on the workspace views surface
 * (VIEW-040): the surface's known layouts, mirroring the old
 * `my_issues` page. The selector shows them in canonical order.
 */
export const CLOUD_WORKSPACE_VIEW_LAYOUTS: readonly string[] = ["list", "spreadsheet"];

/** In-app path of a published view's public read-only page (VIEW-020). */
export function publishedViewPath(anchor: string): string {
  return `/views/published/${encodeURIComponent(anchor)}/`;
}

/** The cloud access seam: the selector plus the storing write (VIEW-019). */
export const cloudViewAccessSlot: ViewAccessSlot = {
  Field: CloudViewAccessField,
  storeAccess,
};

/** The cloud publish seam: the anchor writes plus link derivation (VIEW-020). */
export const cloudViewPublishSlot: ViewPublishSlot = {
  publish,
  unpublish,
  publicLink,
};

/** The cloud workspace layout seam (VIEW-040). */
export const cloudWorkspaceViewLayouts: WorkspaceViewLayoutsSlot = {
  layouts: CLOUD_WORKSPACE_VIEW_LAYOUTS,
};

/**
 * Persist an access choice on the owning scope's view endpoint. A null
 * project id names a workspace view, which spans projects.
 */
async function storeAccess(input: ViewAccessStoreInput): Promise<unknown> {
  const client = getAppClient();
  const echo =
    input.projectId === null
      ? await setWorkspaceViewAccess(client, input.workspaceSlug, input.viewId, input.access)
      : await setProjectViewAccess(client, input.workspaceSlug, input.projectId, input.viewId, input.access);
  if (echo.access !== input.access) {
    throw new Error(VIEW_ACCESS_STORE_FAILED);
  }
  return echo;
}

/**
 * Publish a project view under a fresh opaque token. The client proposes
 * the anchor and the echo confirms it; anything else means the server
 * did not honor the publish.
 */
async function publish(input: ViewPublishInput): Promise<ViewPublishResult> {
  const anchor = crypto.randomUUID();
  const echo = await publishProjectView(getAppClient(), input.workspaceSlug, input.projectId, input.viewId, anchor);
  if (echo.anchor !== anchor) {
    throw new Error(VIEW_PUBLISH_FAILED);
  }
  return { anchor };
}

/** Clear a project view's anchor; anything still live means it did not clear. */
async function unpublish(input: ViewPublishInput): Promise<void> {
  const echo = await unpublishProjectView(getAppClient(), input.workspaceSlug, input.projectId, input.viewId);
  if (isViewLive(echo.anchor)) {
    throw new Error(VIEW_UNPUBLISH_FAILED);
  }
}

/** The public read-only address for a published anchor. */
function publicLink(anchor: string): string {
  return platform.shareableUrl(publishedViewPath(anchor));
}
