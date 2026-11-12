// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { decideAppInstanceGate } from "../../core/instance/status.js";
import * as status from "./status.js";

describe("instance status re-export", () => {
  it("keeps the feature-local path pointing at the shared decision", () => {
    expect(status.decideAppInstanceGate).toBe(decideAppInstanceGate);
  });
});
