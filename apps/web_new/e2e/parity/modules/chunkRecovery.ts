// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared modules-spec navigation guard (NEWFRONT-808): the archives
// chunk loads on client-side navigation, after every driver boot check
// ran, so a net::ERR_NETWORK_CHANGED burst can error the tab entry.
// Retry the entry once past the vite signature only; the retry is a
// full clean re-entry, so a real failure still throws. Spec-land only
// — no archives-owned driver code is touched.
import type { ParityDriver } from "../drivers/parity-driver";

/** Open the archived-modules tab, retrying once past a vite chunk error. */
export async function openArchivesModulesTab(
  driver: ParityDriver,
  workspaceSlug: string,
  projectId: string
): Promise<void> {
  try {
    await driver.archivesOpenModulesTab(workspaceSlug, projectId);
  } catch (error) {
    const body = await driver.page.evaluate(() => document.body?.innerText ?? "").catch(() => "");
    if (!body.includes("Failed to fetch dynamically imported module")) throw error;
    await driver.archivesOpenModulesTab(workspaceSlug, projectId);
  }
}
