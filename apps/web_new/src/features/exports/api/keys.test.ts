// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { exportsKeys } from "./keys.js";

describe("exportsKeys", () => {
  it("scopes the history read under the workspace", () => {
    expect(exportsKeys.all("acme")).toEqual(["ws", "acme", "exports"]);
    // Pinned: NEWFRONT-767 builds its history query on this exact key so
    // the form's post-submit invalidation re-reads it (EXP-006).
    expect(exportsKeys.history("acme")).toEqual(["ws", "acme", "exports", "history"]);
  });
});
