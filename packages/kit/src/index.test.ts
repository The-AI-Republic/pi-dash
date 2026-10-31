// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

describe("@pidash/kit entry", () => {
  it("imports cleanly", async () => {
    const entry = await import("./index");
    expect(entry).toBeDefined();
  });
});
