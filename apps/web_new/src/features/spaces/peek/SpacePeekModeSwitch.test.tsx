// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import {
  SPACE_PEEK_PRESENTATIONS,
  SpacePeekModeSwitch,
  spacePeekPresentationLabel,
  type SpacePeekPresentation,
} from "./SpacePeekModeSwitch.js";

describe("SpacePeekModeSwitch", () => {
  it("labels every presentation", () => {
    expect(SPACE_PEEK_PRESENTATIONS).toEqual(["side", "modal", "full"]);
    expect(SPACE_PEEK_PRESENTATIONS.map((mode) => spacePeekPresentationLabel(mode))).toEqual([
      "Side panel",
      "Modal",
      "Full screen",
    ]);
  });

  it("offers all three presentations and reports the chosen one", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn<(mode: SpacePeekPresentation) => void>();
    render(<SpacePeekModeSwitch value="side" onChange={onChange} />);
    await user.click(screen.getByRole("button", { name: "Change presentation" }));
    expect(await screen.findByRole("menuitem", { name: "Side panel" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Modal" })).toBeInTheDocument();
    await user.click(screen.getByRole("menuitem", { name: "Full screen" }));
    expect(onChange).toHaveBeenCalledWith("full");
  });

  it("keeps the trigger's accessible name in every presentation", () => {
    for (const mode of SPACE_PEEK_PRESENTATIONS) {
      const view = render(<SpacePeekModeSwitch value={mode} onChange={vi.fn()} />);
      expect(screen.getByRole("button", { name: "Change presentation" })).toBeInTheDocument();
      view.unmount();
    }
  });
});
