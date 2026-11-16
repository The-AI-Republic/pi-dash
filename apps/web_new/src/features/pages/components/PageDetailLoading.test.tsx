// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail loading surfaces (PAGE-079): the centered spinner, the body
// skeleton and the snapshot skeleton, each one announced status region.

import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { PageBodySkeleton, PageDetailSpinner, PageVersionSkeleton } from "./PageDetailLoading.js";

describe("PageDetailSpinner", () => {
  it("renders the centered brand mark and announces the wait", () => {
    const { container } = render(<PageDetailSpinner />);
    const root = screen.getByLabelText("Loading page");
    expect(root).toHaveClass("grid", "size-full", "place-items-center");
    expect(root.querySelector("svg.animate-pulse")).not.toBeNull();
    expect(container.querySelectorAll("rect")).toHaveLength(4);
  });
});

describe("page skeletons", () => {
  it("renders body blocks as one status region", () => {
    render(<PageBodySkeleton />);
    const region = screen.getByRole("status", { name: "Loading page body" });
    expect(region).toHaveAttribute("data-testid", "page-body-skeleton");
    expect(region.querySelectorAll("span")).toHaveLength(4);
  });

  it("renders snapshot blocks as one status region", () => {
    render(<PageVersionSkeleton />);
    const region = screen.getByRole("status", { name: "Loading version" });
    expect(region.querySelectorAll("span")).toHaveLength(3);
  });
});
