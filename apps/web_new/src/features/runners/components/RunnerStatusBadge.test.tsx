// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Status badge and rail dot (NEWFRONT-540, RUN-011). The badge carries the
// state name plus its tone; the dot is an aria-hidden SVG in the same tone.
import { render } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { RUNNER_STATUSES } from "../status.js";
import { RunnerStatusBadge, RunnerStatusDot, runnerStatusOf } from "./RunnerStatusBadge.js";

describe("RunnerStatusBadge", () => {
  it("labels every state", () => {
    for (const status of RUNNER_STATUSES) {
      const { container, unmount } = render(<RunnerStatusBadge status={status} />);
      expect(container.textContent).toBe(status);
      unmount();
    }
  });

  it("exposes the label through a stable test hook", () => {
    const { container } = render(<RunnerStatusBadge status="busy" />);
    expect(container.querySelector("[data-testid='runner-status-badge']")).toHaveTextContent("busy");
  });

  it("paints each state in its own tone through the kit badge", () => {
    const tones = RUNNER_STATUSES.map((status) => {
      const { container, unmount } = render(<RunnerStatusBadge status={status} />);
      const dot = container.querySelector("span[aria-hidden='true']") as HTMLElement;
      const classes = dot.className;
      unmount();
      return classes;
    });
    expect(new Set(tones).size).toBe(RUNNER_STATUSES.length);
    expect(tones[0]).toContain("bg-(--success)");
  });
});

describe("RunnerStatusDot", () => {
  it("renders a decorative SVG circle in the state's tone", () => {
    const { container } = render(<RunnerStatusDot status="revoked" />);
    const svg = container.querySelector("svg");
    expect(svg).not.toBeNull();
    expect(svg!.getAttribute("aria-hidden")).toBe("true");
    expect(svg!.getAttribute("data-status")).toBe("revoked");
    expect(svg!.className.baseVal).toContain("text-(--warning)");
    expect(svg!.querySelector("circle")).not.toBeNull();
  });
});

describe("runnerStatusOf", () => {
  it("reads a never-enrolled row as offline", () => {
    expect(runnerStatusOf({ status: "online", enrolled_at: null })).toBe("offline");
    expect(runnerStatusOf({ status: "busy", enrolled_at: "2024-01-01T00:00:00Z" })).toBe("busy");
  });
});
