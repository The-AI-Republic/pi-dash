// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public surface of the cloud assistant feature: other features and routes
// import from the barrel, so it must keep exporting what they use.
import { describe, expect, it } from "vitest";

import * as cloudAssistant from "./index.js";

describe("cloud-assistant barrel", () => {
  it("exports the page, its registration, and the lane-adjacent helpers", () => {
    expect(typeof cloudAssistant.CloudAssistantTab).toBe("function");
    expect(typeof cloudAssistant.registerCloudAssistantSettings).toBe("function");
    expect(typeof cloudAssistant.AppsPointer).toBe("function");
    expect(typeof cloudAssistant.useProviderSignIn).toBe("function");
    expect(typeof cloudAssistant.useCloudAssistantConfig).toBe("function");
    expect(typeof cloudAssistant.useCloudConnections).toBe("function");
    expect(typeof cloudAssistant.connectionTitle).toBe("function");
    expect(typeof cloudAssistant.beginProviderSignIn).toBe("function");
    expect(typeof cloudAssistant.finishNotice).toBe("function");
  });

  it("keeps the query keys and poll constants stable for consumers", () => {
    expect(cloudAssistant.cloudAssistantKeys.all).toEqual(["cloud-assistant"]);
    expect(cloudAssistant.SIGN_IN_POLL_INTERVAL_MS).toBe(2_000);
    expect(cloudAssistant.SIGN_IN_POLL_TIMEOUT_MS).toBe(300_000);
  });
});
