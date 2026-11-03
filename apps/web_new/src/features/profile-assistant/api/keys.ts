// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Query keys for the AI-assistant settings tab (Data layer). These reads
// belong to the signed-in user rather than a workspace, so they sit under
// one user-level subtree that sign-out drops with the rest of the cache.

export const assistantSettingsKeys = {
  all: ["profile-ai-assistant"] as const,
  llm: ["profile-ai-assistant", "llm"] as const,
  voice: ["profile-ai-assistant", "voice"] as const,
  servers: ["profile-ai-assistant", "servers"] as const,
};
