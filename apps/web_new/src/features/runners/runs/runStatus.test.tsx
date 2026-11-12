// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared run-status badge (NEWFRONT-545, RUN-022). Every status in the
// product vocabulary gets its readable label and tone; a retired status
// still renders its raw token on a neutral badge.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { RunStatusBadge } from "./RunStatusBadge.js";
import { RUN_STATUS_LABELS, RUN_STATUS_TONES, runStatusLabel, runStatusTone } from "./runStatus.js";

const EXPECTED: Array<[string, string]> = [
  ["queued", "queued"],
  ["assigned", "assigned"],
  ["running", "running"],
  ["cancel_requested", "cancellation requested"],
  ["awaiting_approval", "awaiting approval"],
  ["awaiting_reauth", "awaiting reauth"],
  ["paused_awaiting_input", "paused awaiting input"],
  ["blocked", "blocked"],
  ["completed", "completed"],
  ["failed", "failed"],
  ["cancelled", "cancelled"],
  ["refused", "refused"],
];

describe("run status mapping", () => {
  it("labels the full status set", () => {
    for (const [status, label] of EXPECTED) {
      expect(runStatusLabel(status)).toBe(label);
    }
    expect(Object.keys(RUN_STATUS_LABELS)).toHaveLength(EXPECTED.length);
  });

  it("tones each state and shares warning across unsettled states", () => {
    expect(runStatusTone("queued")).toBe("neutral");
    expect(runStatusTone("assigned")).toBe("accent");
    expect(runStatusTone("running")).toBe("accent");
    expect(runStatusTone("cancel_requested")).toBe("warning");
    expect(runStatusTone("awaiting_approval")).toBe("warning");
    expect(runStatusTone("awaiting_reauth")).toBe("warning");
    expect(runStatusTone("paused_awaiting_input")).toBe("warning");
    expect(runStatusTone("blocked")).toBe("warning");
    expect(runStatusTone("completed")).toBe("success");
    expect(runStatusTone("failed")).toBe("danger");
    expect(runStatusTone("cancelled")).toBe("neutral");
    expect(runStatusTone("refused")).toBe("danger");
    expect(Object.keys(RUN_STATUS_TONES)).toHaveLength(EXPECTED.length);
  });

  it("falls back to the raw token and neutral for an unknown status", () => {
    expect(runStatusLabel("waiting_for_worktree")).toBe("waiting_for_worktree");
    expect(runStatusTone("waiting_for_worktree")).toBe("neutral");
  });
});

describe("RunStatusBadge", () => {
  it("renders the readable label for a known status", () => {
    render(<RunStatusBadge status="cancel_requested" />);
    expect(screen.getByText("cancellation requested")).toBeInTheDocument();
  });

  it("renders an unknown status verbatim without crashing", () => {
    render(<RunStatusBadge status="waiting_for_worktree" />);
    expect(screen.getByText("waiting_for_worktree")).toBeInTheDocument();
  });
});
