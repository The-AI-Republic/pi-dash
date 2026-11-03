// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { GroupHeader } from "./GroupHeader.js";
import { StateMarker } from "./StateMarker.js";

describe("StateMarker", () => {
  it("uses the glyph for the state's group and the state's colour", () => {
    render(<StateMarker state={{ name: "Todo", color: "#a00", group: "unstarted" }} />);
    const marker = screen.getByTestId("space-state-marker");
    expect(marker).toHaveAttribute("title", "Todo");
    expect(marker).toHaveAttribute("style", expect.stringContaining("rgb(170, 0, 0)"));
  });

  it("keeps a marker for a group it has no glyph for", () => {
    render(<StateMarker state={{ name: "Odd", color: "#00a", group: "unknown-group" as never }} />);
    expect(screen.getByTestId("space-state-marker")).toBeInTheDocument();
  });

  it("is decorative: the name is not carried by the marker alone", () => {
    render(<StateMarker state={{ name: "Todo", color: "#000", group: "unstarted" }} />);
    expect(screen.getByTestId("space-state-marker")).toHaveAttribute("aria-hidden", "true");
  });
});

describe("GroupHeader", () => {
  it("shows the marker, the name and the count without a toggle", () => {
    render(<GroupHeader state={{ name: "Todo", color: "#000", group: "unstarted" }} count={0} />);
    expect(screen.getByTestId("space-group-name")).toHaveTextContent("Todo");
    expect(screen.getByTestId("space-group-count")).toHaveTextContent("0");
    expect(screen.queryByTestId("space-list-group-header")).not.toBeInTheDocument();
  });

  it("announces its expanded state and toggles on press", async () => {
    const onToggle = vi.fn();
    const { rerender } = render(
      <GroupHeader state={{ name: "Todo", color: "#000", group: "unstarted" }} count={2} expanded onToggle={onToggle} />
    );
    const header = screen.getByTestId("space-list-group-header");
    expect(header).toHaveAttribute("aria-expanded", "true");
    await userEvent.click(header);
    expect(onToggle).toHaveBeenCalledTimes(1);
    rerender(
      <GroupHeader
        state={{ name: "Todo", color: "#000", group: "unstarted" }}
        count={2}
        expanded={false}
        onToggle={onToggle}
      />
    );
    expect(header).toHaveAttribute("aria-expanded", "false");
  });

  it("treats a header without an expanded flag as collapsed", () => {
    render(
      <GroupHeader state={{ name: "Todo", color: "#000", group: "unstarted" }} count={2} onToggle={() => undefined} />
    );
    expect(screen.getByTestId("space-list-group-header")).toHaveAttribute("aria-expanded", "false");
  });
});
