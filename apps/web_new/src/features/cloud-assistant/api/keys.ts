// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud assistant query keys (Data layer). Connections and the provider
// config belong to the signed-in user, not a workspace, so the keys carry
// no workspace scope: the same answer backs the settings tab and every
// assistant entry point.

export const cloudAssistantKeys = {
  all: ["cloud-assistant"] as const,
  connections: () => [...cloudAssistantKeys.all, "connections"] as const,
  config: () => [...cloudAssistantKeys.all, "config"] as const,
};
