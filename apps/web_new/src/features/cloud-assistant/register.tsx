// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Edition registration for the AI Assistant tab (CLOUD-032; maintainer
// decision of 2026-10-18). When the edition declares the `ai-assistant`
// settings section, this feature owns that tab's body; the OSS edition
// declares nothing and keeps the edition-neutral assistant tab. The tab id
// itself is part of the profile area's seam, so this module registers
// through it rather than replacing it.

import * as React from "react";

import { edition, isEditionSettingsSectionEnabled } from "../../core/edition/index.js";
import { registerProfileSettingsPage } from "../settings/index.js";
import { CloudAssistantTab } from "./components/CloudAssistantTab.js";

/** The settings section id this slice mounts. */
export const CLOUD_ASSISTANT_SECTION_ID = "ai-assistant";

/**
 * Register the cloud tab body when the edition declares its section.
 * Returns the unregister callback, or a no-op for editions that leave the
 * tab to the community implementation.
 */
export function registerCloudAssistantSettings(active = edition, register = registerProfileSettingsPage): () => void {
  if (!isEditionSettingsSectionEnabled(active, CLOUD_ASSISTANT_SECTION_ID)) return () => undefined;
  return register(CLOUD_ASSISTANT_SECTION_ID, () => <CloudAssistantTab />);
}
