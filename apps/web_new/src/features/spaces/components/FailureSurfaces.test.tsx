// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { FORUM_URL } from "../../../shared/shell/externalLinks.js";
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  SPACE_ANCHOR_NOT_FOUND_ART,
  SPACE_BOARD_ERROR_ART,
  SpaceAnchorNotFound,
  SpaceBoardError,
} from "./FailureSurfaces.js";
import { reloadSpaceCrash, SpaceCrashScreen, SPACE_CRASH_HEADING, SPACE_SUPPORT_HREF } from "./CrashScreen.js";
import {
  reloadSpacePage,
  SpaceInstanceFailureScreen,
  SPACE_INSTANCE_FAILURE_ART,
  SPACE_INSTANCE_FAILURE_HEADING,
} from "./InstanceFailureScreen.js";

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("SpaceAnchorNotFound", () => {
  it("renders the anchor illustration and no board chrome", () => {
    render(<SpaceAnchorNotFound />);
    expect(screen.getByTestId("space-anchor-not-found")).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 3, name: "We couldn't find this board" })).toBeInTheDocument();
    expect(screen.getByRole("img")).toHaveAttribute("src", SPACE_ANCHOR_NOT_FOUND_ART);
    expect(document.querySelector("header")).toBeNull();
    expect(document.title).toContain("Board not found");
  });
});

describe("SpaceBoardError", () => {
  it("renders the board-error illustration with a primary heading", () => {
    render(<SpaceBoardError />);
    expect(screen.getByTestId("space-board-error")).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 1, name: "We couldn't load this board" })).toBeInTheDocument();
    expect(screen.getByRole("img")).toHaveAttribute("src", SPACE_BOARD_ERROR_ART);
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });
});

describe("SpaceInstanceFailureScreen", () => {
  it("shows the wordmark, illustration, explanation and retry reload", () => {
    const reload = vi.fn();
    vi.stubGlobal("window", { location: { reload } });
    render(<SpaceInstanceFailureScreen />);
    expect(screen.getByTestId("space-instance-failure")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: SPACE_INSTANCE_FAILURE_HEADING })).toBeInTheDocument();
    expect(document.querySelector(`img[src="${SPACE_INSTANCE_FAILURE_ART}"]`)).not.toBeNull();
    expect(screen.getByRole("link")).toHaveAttribute("href", "/");
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(reload).toHaveBeenCalledTimes(1);
    reloadSpacePage();
    expect(reload).toHaveBeenCalledTimes(2);
  });
});

describe("SpaceCrashScreen", () => {
  it("offers refresh plus support and forum contacts", () => {
    const reload = vi.fn();
    vi.stubGlobal("window", { location: { reload } });
    render(<SpaceCrashScreen />);
    expect(screen.getByTestId("space-crash")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: SPACE_CRASH_HEADING })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Email support" })).toHaveAttribute("href", SPACE_SUPPORT_HREF);
    expect(screen.getByRole("link", { name: "Community forum" })).toHaveAttribute("href", FORUM_URL);
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(reload).toHaveBeenCalledTimes(1);
    reloadSpaceCrash();
    expect(reload).toHaveBeenCalledTimes(2);
  });
});
