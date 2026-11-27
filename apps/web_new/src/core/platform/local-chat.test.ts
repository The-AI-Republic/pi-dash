// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { hasLocalChat } from "./local-chat.js";

describe("hasLocalChat", () => {
  it("is false on the web platform (vitest resolves platform-target to web)", () => {
    // The route branch reads the same seam (NEWPIDASH-300): web stays on
    // the server assistant and never loads the local chat transport.
    expect(hasLocalChat()).toBe(false);
  });
});
