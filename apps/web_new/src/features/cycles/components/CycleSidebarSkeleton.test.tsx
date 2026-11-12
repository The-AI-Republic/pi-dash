// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared sidebar skeleton: the surface test id, the loading role, and
// the placeholder bars behind both mounts.

import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { CycleSidebarSkeleton } from "./CycleSidebarSkeleton.js";

describe("CycleSidebarSkeleton", () => {
  it("announces loading under the surface test id", () => {
    render(<CycleSidebarSkeleton testId="cycle-sidebar-skeleton" />);
    const skeleton = screen.getByTestId("cycle-sidebar-skeleton");
    expect(skeleton.getAttribute("role")).toBe("status");
    expect(skeleton.getAttribute("aria-label")).toBe("Loading cycle details");
  });

  it("renders under the peek test id too", () => {
    render(<CycleSidebarSkeleton testId="cycle-peek-skeleton" />);
    expect(screen.getByTestId("cycle-peek-skeleton")).not.toBeNull();
  });
});
