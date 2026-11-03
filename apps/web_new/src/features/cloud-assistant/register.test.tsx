// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Edition registration (CLOUD-032): the cloud body takes the AI Assistant
// tab only where the edition declares that section, and an OSS build keeps
// the community tab's registration untouched.
import { describe, expect, it, vi } from "vitest";

import type { Edition } from "../../core/edition/types.js";
import { CLOUD_ASSISTANT_SECTION_ID, registerCloudAssistantSettings } from "./register.js";

const cloud: Edition = {
  id: "cloud",
  flags: {},
  settingsSections: [{ id: "ai-assistant", label: "AI Assistant" }],
};

const oss: Edition = { id: "oss", flags: {} };

describe("registerCloudAssistantSettings", () => {
  it("mounts the tab where the edition declares the assistant section", () => {
    const unregister = vi.fn();
    const register = vi.fn(() => unregister);
    const stop = registerCloudAssistantSettings(cloud, register);
    expect(register).toHaveBeenCalledWith(CLOUD_ASSISTANT_SECTION_ID, expect.any(Function));
    stop();
    expect(unregister).toHaveBeenCalledTimes(1);
  });

  it("leaves the tab alone on an edition without the section", () => {
    const register = vi.fn(() => () => undefined);
    const stop = registerCloudAssistantSettings(oss, register);
    expect(register).not.toHaveBeenCalled();
    expect(() => stop()).not.toThrow();
  });
});
