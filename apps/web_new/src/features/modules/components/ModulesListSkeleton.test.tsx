// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The skeleton matches the active layout (MOD-008): rows for list, cards
// for board, name bars for timeline, and never the shapes of another
// layout.

import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { ModulesListSkeleton } from "./ModulesListSkeleton.js";

describe("ModulesListSkeleton", () => {
  it("renders row shapes for the list layout", () => {
    const { container } = render(<ModulesListSkeleton layout="list" />);
    const root = screen.getByTestId("modules-list-skeleton");
    expect(root).toHaveAttribute("data-layout", "list");
    expect(root).toHaveAttribute("role", "status");
    expect(container.querySelectorAll("[data-skeleton-row]")).toHaveLength(6);
    expect(container.querySelector("[data-skeleton-card]")).toBeNull();
    expect(container.querySelector("[data-skeleton-bar-row]")).toBeNull();
  });

  it("renders card shapes for the board layout", () => {
    const { container } = render(<ModulesListSkeleton layout="board" />);
    expect(screen.getByTestId("modules-list-skeleton")).toHaveAttribute("data-layout", "board");
    expect(container.querySelectorAll("[data-skeleton-card]")).toHaveLength(6);
    expect(container.querySelector("[data-skeleton-row]")).toBeNull();
  });

  it("renders bar rows for the timeline layout", () => {
    const { container } = render(<ModulesListSkeleton layout="timeline" />);
    expect(screen.getByTestId("modules-list-skeleton")).toHaveAttribute("data-layout", "timeline");
    expect(container.querySelectorAll("[data-skeleton-bar-row]")).toHaveLength(6);
    expect(container.querySelector("[data-skeleton-row]")).toBeNull();
  });

  it("names its loading state for assistive tech", () => {
    render(<ModulesListSkeleton layout="list" />);
    expect(screen.getByText("Loading modules")).toBeInTheDocument();
  });
});
