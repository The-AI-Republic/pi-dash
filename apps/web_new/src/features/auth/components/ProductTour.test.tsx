// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { TOUR_VIDEO_EMBED_URL } from "../tour.js";
import {
  ProductTour,
  TOUR_CLOSE_LABEL,
  TOUR_SKIP_LABEL,
  TOUR_START_PROJECT_LABEL,
  TOUR_TAKE_TOUR_LABEL,
} from "./ProductTour.js";

function renderTour(options: { firstName?: string; onCreateProject?: () => void } = {}) {
  const onComplete = vi.fn();
  const onCreateProject = options.onCreateProject ? vi.fn(options.onCreateProject) : undefined;
  render(
    <ProductTour
      firstName={options.firstName ?? "Ada"}
      onComplete={onComplete}
      {...(onCreateProject ? { onCreateProject } : {})}
    />
  );
  return { onComplete, onCreateProject };
}

describe("ProductTour", () => {
  it("welcomes the user by name with the concept video and both exits", () => {
    renderTour();
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Welcome to Pi Dash, Ada" })).toBeInTheDocument();
    expect(screen.getByTitle("Pi Dash core concept video")).toHaveAttribute("src", TOUR_VIDEO_EMBED_URL);
    expect(screen.getByRole("button", { name: TOUR_TAKE_TOUR_LABEL })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: TOUR_SKIP_LABEL })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: TOUR_CLOSE_LABEL })).not.toBeInTheDocument();
  });

  it("greets unnamed users plainly", () => {
    renderTour({ firstName: "" });
    expect(screen.getByRole("heading", { name: "Welcome to Pi Dash" })).toBeInTheDocument();
  });

  it("skips the tour through the explore action", async () => {
    const user = userEvent.setup();
    const { onComplete } = renderTour();
    await user.click(screen.getByRole("button", { name: TOUR_SKIP_LABEL }));
    expect(onComplete).toHaveBeenCalledTimes(1);
  });

  it("walks the stops with Back and Next and closes from any stop", async () => {
    const user = userEvent.setup();
    const { onComplete } = renderTour();
    await user.click(screen.getByRole("button", { name: TOUR_TAKE_TOUR_LABEL }));

    // First stop: Next only, no way back to welcome.
    expect(screen.getByRole("heading", { name: "Give every task a home" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Back" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Next" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: TOUR_START_PROJECT_LABEL })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(screen.getByRole("heading", { name: "Put agents to work on your machine" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Back" }));
    expect(screen.getByRole("heading", { name: "Give every task a home" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: TOUR_CLOSE_LABEL }));
    expect(onComplete).toHaveBeenCalledTimes(1);
  });

  it("jumps between stops from the navigator with the current stop marked", async () => {
    const user = userEvent.setup();
    renderTour();
    await user.click(screen.getByRole("button", { name: TOUR_TAKE_TOUR_LABEL }));
    expect(screen.getByRole("button", { name: "Work items" })).toHaveAttribute("aria-current", "step");

    await user.click(screen.getByRole("button", { name: "Pages" }));
    expect(screen.getByRole("heading", { name: "Keep knowledge where agents find it" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Pages" })).toHaveAttribute("aria-current", "step");
    expect(screen.getByRole("button", { name: "Work items" })).not.toHaveAttribute("aria-current");
  });

  it("ends the last stop in the project action before the dialog opener", async () => {
    const user = userEvent.setup();
    const calls: string[] = [];
    const { onComplete, onCreateProject } = renderTour({
      onCreateProject: () => {
        calls.push("open");
      },
    });
    onComplete.mockImplementation(() => {
      calls.push("complete");
    });
    await user.click(screen.getByRole("button", { name: TOUR_TAKE_TOUR_LABEL }));
    await user.click(screen.getByRole("button", { name: "Pages" }));

    expect(screen.queryByRole("button", { name: "Next" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Back" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: TOUR_START_PROJECT_LABEL }));
    expect(calls).toEqual(["complete", "open"]);
    expect(onCreateProject).toHaveBeenCalledTimes(1);
  });

  it("finishes the last stop without a dialog opener until the projects area lands", async () => {
    const user = userEvent.setup();
    const { onComplete } = renderTour();
    await user.click(screen.getByRole("button", { name: TOUR_TAKE_TOUR_LABEL }));
    await user.click(screen.getByRole("button", { name: "Pages" }));
    await user.click(screen.getByRole("button", { name: TOUR_START_PROJECT_LABEL }));
    expect(onComplete).toHaveBeenCalledTimes(1);
  });
});
