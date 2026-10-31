// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { assistantKeys } from "./keys.js";

describe("assistantKeys", () => {
  it("scopes every key under the workspace and the assistant subtree", () => {
    expect(assistantKeys.all("acme")).toEqual(["ws", "acme", "assistant"]);
    expect(assistantKeys.threads("acme")).toEqual(["ws", "acme", "assistant", "threads"]);
    expect(assistantKeys.config("acme")).toEqual(["ws", "acme", "assistant", "config"]);
  });
});
