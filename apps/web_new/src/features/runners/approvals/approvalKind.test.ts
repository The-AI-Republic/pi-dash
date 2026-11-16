// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Queue headings per approval kind (NEWFRONT-547, RUN-023).
import { describe, expect, it } from "vitest";

import { APPROVAL_KIND_LABELS, approvalKindLabel } from "./approvalKind.js";

describe("approvalKindLabel", () => {
  it("labels every contract kind", () => {
    expect(APPROVAL_KIND_LABELS).toEqual({
      command_execution: "Run a shell command",
      file_change: "Change a file",
      network_access: "Reach the network",
      other: "Needs your approval",
    });
    expect(approvalKindLabel("command_execution")).toBe("Run a shell command");
    expect(approvalKindLabel("file_change")).toBe("Change a file");
    expect(approvalKindLabel("network_access")).toBe("Reach the network");
    expect(approvalKindLabel("other")).toBe("Needs your approval");
  });
});
