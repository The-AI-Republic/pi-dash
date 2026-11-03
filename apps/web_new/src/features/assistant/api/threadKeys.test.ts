// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { assistantKeys } from "./keys.js";
import { threadKey, transcriptKey } from "./threadKeys.js";

describe("thread keys", () => {
  it("nests one conversation and its transcript under the assistant subtree", () => {
    expect(threadKey("acme", "t1")).toEqual(["ws", "acme", "assistant", "thread", "t1"]);
    expect(transcriptKey("acme", "t1")).toEqual(["ws", "acme", "assistant", "thread", "t1", "messages"]);
  });

  it("invalidates the thread subtree through the workspace root", () => {
    expect(transcriptKey("acme", "t1").slice(0, assistantKeys.all("acme").length)).toEqual(assistantKeys.all("acme"));
  });
});
