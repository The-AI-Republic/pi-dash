// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { INTAKE_PAGING_THRESHOLD_PX, nearListEnd } from "./paging.js";

describe("intake paging trigger (INT-007)", () => {
  it("fires within two hundred pixels of the end", () => {
    expect(INTAKE_PAGING_THRESHOLD_PX).toBe(200);
    expect(nearListEnd({ scrollHeight: 1000, scrollTop: 800, clientHeight: 200 })).toBe(true);
    expect(nearListEnd({ scrollHeight: 1000, scrollTop: 799, clientHeight: 200 })).toBe(true);
    expect(nearListEnd({ scrollHeight: 1000, scrollTop: 300, clientHeight: 200 })).toBe(false);
  });
});
