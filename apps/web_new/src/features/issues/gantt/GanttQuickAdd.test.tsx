// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline quick-add (parity/specs/issues.md §6, ISS-055): absent
// without standing or in a completed cycle, opens an inline field,
// submits a trimmed title, clears and closes on success, and ignores an
// empty submit.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { GanttQuickAdd } from "./GanttQuickAdd.js";

describe("GanttQuickAdd", () => {
  it("is absent without create standing", () => {
    render(<GanttQuickAdd enabled={false} onCreate={vi.fn()} />);
    expect(screen.queryByTestId("gantt-quick-add")).not.toBeInTheDocument();
  });

  it("is absent in a completed cycle", () => {
    render(<GanttQuickAdd enabled completedCycle onCreate={vi.fn()} />);
    expect(screen.queryByTestId("gantt-quick-add")).not.toBeInTheDocument();
  });

  it("is absent in a completed module", () => {
    render(<GanttQuickAdd enabled completedModule onCreate={vi.fn()} />);
    expect(screen.queryByTestId("gantt-quick-add")).not.toBeInTheDocument();
  });

  it("opens the field, submits the trimmed title and closes", async () => {
    const onCreate = vi.fn(async () => undefined);
    render(<GanttQuickAdd enabled onCreate={onCreate} />);
    fireEvent.click(screen.getByText("New work item"));
    const field = screen.getByPlaceholderText("Work item title");
    fireEvent.change(field, { target: { value: "  Fresh bar  " } });
    fireEvent.submit(screen.getByTestId("gantt-quick-add-form"));
    await waitFor(() => expect(onCreate).toHaveBeenCalledWith("Fresh bar"));
    await waitFor(() => expect(screen.queryByTestId("gantt-quick-add-form")).not.toBeInTheDocument());
  });

  it("ignores an empty submit and Escape closes the field", () => {
    const onCreate = vi.fn();
    render(<GanttQuickAdd enabled onCreate={onCreate} />);
    fireEvent.click(screen.getByText("New work item"));
    fireEvent.submit(screen.getByTestId("gantt-quick-add-form"));
    expect(onCreate).not.toHaveBeenCalled();
    fireEvent.keyDown(screen.getByPlaceholderText("Work item title"), { key: "Escape" });
    expect(screen.queryByTestId("gantt-quick-add-form")).not.toBeInTheDocument();
  });

  it("stays open and keeps the entry quiet when the create rejects", async () => {
    const onCreate = vi.fn(async () => {
      throw new Error("refused");
    });
    render(<GanttQuickAdd enabled onCreate={onCreate} />);
    fireEvent.click(screen.getByText("New work item"));
    fireEvent.change(screen.getByPlaceholderText("Work item title"), { target: { value: "Bar" } });
    fireEvent.submit(screen.getByTestId("gantt-quick-add-form"));
    await waitFor(() => expect(onCreate).toHaveBeenCalled());
    expect(screen.getByTestId("gantt-quick-add-form")).toBeInTheDocument();
    expect(screen.getByPlaceholderText("Work item title")).toHaveValue("Bar");
  });

  it("clears the title only after a successful create", async () => {
    let resolve: (() => void) | null = null;
    const onCreate = vi.fn(
      () =>
        new Promise<void>((done) => {
          resolve = done;
        })
    );
    render(<GanttQuickAdd enabled onCreate={onCreate} />);
    fireEvent.click(screen.getByText("New work item"));
    const field = screen.getByPlaceholderText("Work item title");
    fireEvent.change(field, { target: { value: "Bar" } });
    fireEvent.submit(screen.getByTestId("gantt-quick-add-form"));
    await waitFor(() => expect(onCreate).toHaveBeenCalled());
    expect(field).toHaveValue("Bar");
    resolve!();
    await waitFor(() => expect(screen.queryByTestId("gantt-quick-add-form")).not.toBeInTheDocument());
  });
});
