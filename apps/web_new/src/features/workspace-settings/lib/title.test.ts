// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Tab titles and the webhook detail address (WSET-007/036, spec §2/§36).
// The oracle compares against "<workspace> - <section>" exactly.
import { describe, expect, it } from "vitest";

import { isWorkspaceWebhookDetailPath, workspaceSettingsTitle } from "./title.js";

describe("workspaceSettingsTitle", () => {
  it("names the workspace and each section", () => {
    const cases = [
      ["general", "General settings"],
      ["members", "Members"],
      ["billing", "Billing & Plans"],
      ["exports", "Exports"],
      ["webhooks", "Webhooks"],
      ["integrations", "Integrations"],
    ] as const;
    for (const [section, label] of cases) {
      expect(workspaceSettingsTitle({ workspaceName: "Acme", section, webhookDetail: false })).toBe(`Acme - ${label}`);
    }
  });

  it("names a webhook detail address even under the webhooks section", () => {
    expect(workspaceSettingsTitle({ workspaceName: "Acme", section: "webhooks", webhookDetail: true })).toBe(
      "Acme - Webhook"
    );
  });
});

describe("isWorkspaceWebhookDetailPath", () => {
  it("recognises an id segment after the webhooks list", () => {
    expect(isWorkspaceWebhookDetailPath("/acme/settings/webhooks/abc")).toBe(true);
    expect(isWorkspaceWebhookDetailPath("/acme/settings/webhooks/")).toBe(false);
    expect(isWorkspaceWebhookDetailPath("/acme/settings/webhooks")).toBe(false);
    expect(isWorkspaceWebhookDetailPath("/acme/settings/members/abc")).toBe(false);
    expect(isWorkspaceWebhookDetailPath("/settings/webhooks/abc")).toBe(false);
  });
});
