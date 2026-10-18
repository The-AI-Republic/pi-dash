// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { ListPeek } from "./ListPeek.js";

describe("ListPeek", () => {
  it("shows the key and title and closes from the control", async () => {
    const onClose = vi.fn();
    const user = userEvent.setup();
    render(<ListPeek issueKey="WEB-42" title="Fix login" onClose={onClose} />);
    expect(screen.getByLabelText("Work item preview")).toBeInTheDocument();
    expect(screen.getByText("WEB-42")).toBeInTheDocument();
    expect(screen.getByLabelText("Work item title")).toHaveValue("Fix login");
    await user.click(screen.getByRole("button", { name: "Close preview" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("closes on Escape and ignores other keys", () => {
    const onClose = vi.fn();
    render(<ListPeek issueKey="WEB-42" title="Fix login" onClose={onClose} />);
    fireEvent.keyDown(document, { key: "Enter" });
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("removes the key listener on unmount", () => {
    const onClose = vi.fn();
    const { unmount } = render(<ListPeek issueKey="WEB-42" title="Fix login" onClose={onClose} />);
    unmount();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();
  });
});
