// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { BULK_OPERATIONS_FLAG, isBulkOperationsEnabled } from "./edition.js";

describe("bulk operations edition seam", () => {
  it("is off for an empty community flag set", () => {
    expect(isBulkOperationsEnabled({})).toBe(false);
    expect(BULK_OPERATIONS_FLAG).toBe("issue-bulk-operations");
  });

  it("turns on only for an explicit true flag", () => {
    expect(isBulkOperationsEnabled({ [BULK_OPERATIONS_FLAG]: true })).toBe(true);
    expect(isBulkOperationsEnabled({ [BULK_OPERATIONS_FLAG]: false })).toBe(false);
  });
});
