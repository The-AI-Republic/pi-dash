// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import appPackage from "../../../package.json";
import { APP_VERSION } from "./version.js";

describe("APP_VERSION", () => {
  it("tracks the app package's declared version", () => {
    expect(APP_VERSION).toBe(appPackage.version);
    expect(APP_VERSION).toMatch(/^\d+\.\d+\.\d+/);
  });
});
