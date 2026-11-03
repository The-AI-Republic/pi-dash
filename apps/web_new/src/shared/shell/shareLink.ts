// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Copying an in-app address (DESK-019, parity/specs/desktop.md §5). One
// helper for every copy-link path: the active platform turns the path into
// the address that belongs in a browser (the hosted app on desktop, the
// page origin on web), the clipboard write is reported through the shared
// toasts, and a refusal — no hosted origin configured, a non-web base, or a
// denied clipboard — surfaces as the failure toast instead of a bad link.

import { platform } from "../../core/platform/index.js";
import { notifyError, notifySuccess } from "./toaster.js";

export interface CopyLinkMessages {
  /** Toast title when the write lands. */
  copied: string;
  /** Toast title when the address cannot be produced or written. */
  failed: string;
  /** Optional toast body for the success case. */
  copiedBody?: string;
  /**
   * Optional toast body for a refusal. When omitted the underlying reason
   * (no hosted origin, denied clipboard) is shown instead.
   */
  failedBody?: string;
}

const CLIPBOARD_UNAVAILABLE = "The clipboard is not available in this window.";

/** The address a share action should copy for `path`; throws when it cannot. */
export function shareableAddress(path: string): string {
  return platform.shareableUrl(path);
}

/** Open `path`'s shareable address through the active platform. Rejects on failure. */
export async function openShareAddress(path: string): Promise<void> {
  await platform.openExternal(shareableAddress(path));
}

/** Write `path`'s shareable address to the clipboard. Rejects on failure. */
export async function copyShareAddress(path: string): Promise<string> {
  const address = shareableAddress(path);
  const clipboard = globalThis.navigator?.clipboard;
  if (clipboard === undefined) throw new Error(CLIPBOARD_UNAVAILABLE);
  await clipboard.writeText(address);
  return address;
}

/**
 * Copy with toast reporting. Never rejects: the caller is a menu entry or a
 * button whose only remaining job is to return. Resolves true when written.
 */
export async function copyLinkWithToast(
  path: string,
  messages: CopyLinkMessages,
  notify: {
    success(title: string, body?: string): void;
    error(title: string, body?: string): void;
  }
): Promise<boolean> {
  try {
    await copyShareAddress(path);
    notify.success(messages.copied, messages.copiedBody);
    return true;
  } catch (error) {
    notify.error(messages.failed, messages.failedBody ?? (error instanceof Error ? error.message : undefined));
    return false;
  }
}

/** Copy a shareable address, reporting through the app toasts. */
export async function copyWorkItemLink(path: string, messages: CopyLinkMessages): Promise<boolean> {
  return copyLinkWithToast(path, messages, { success: notifySuccess, error: notifyError });
}

/** The canonical detail address for a work item inside a workspace. */
export function workItemPath(workspaceSlug: string, issueKey: string): string {
  return `/${workspaceSlug}/browse/${issueKey}/`;
}
