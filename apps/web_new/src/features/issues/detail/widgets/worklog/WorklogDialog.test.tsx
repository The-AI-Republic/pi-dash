// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { parseMinutesInput, todayLocalISO, WorklogDialog } from "./WorklogDialog.js";

describe("todayLocalISO", () => {
  it("renders the local calendar day as YYYY-MM-DD", () => {
    expect(todayLocalISO(new Date(2026, 9, 20, 15, 30))).toBe("2026-10-20");
    expect(todayLocalISO(new Date(2026, 0, 5))).toBe("2026-01-05");
  });
});

describe("parseMinutesInput", () => {
  it("accepts positive whole numbers", () => {
    expect(parseMinutesInput("90")).toBe(90);
    expect(parseMinutesInput("  45 ")).toBe(45);
    expect(parseMinutesInput("007")).toBe(7);
  });

  it("rejects zero, fractions, signs and non-numbers", () => {
    expect(parseMinutesInput("0")).toBeNull();
    expect(parseMinutesInput("")).toBeNull();
    expect(parseMinutesInput("1.5")).toBeNull();
    expect(parseMinutesInput("-30")).toBeNull();
    expect(parseMinutesInput("an hour")).toBeNull();
  });
});

describe("WorklogDialog", () => {
  it("logs fresh time with a trimmed note", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    const onClose = vi.fn();
    render(<WorklogDialog saving={false} onClose={onClose} onSubmit={onSubmit} />);

    expect(screen.getByRole("heading", { name: "Log time" })).toBeDefined();
    const save = screen.getByRole("button", { name: "Log time" });

    await user.type(screen.getByLabelText("Minutes"), "90");
    await user.clear(screen.getByLabelText("Date"));
    await user.type(screen.getByLabelText("Date"), "2026-10-20");
    await user.type(screen.getByLabelText("Note"), "  Debugging the flake  ");

    await user.click(save);
    expect(onSubmit).toHaveBeenCalledWith({ minutes: 90, logged_on: "2026-10-20", note: "Debugging the flake" });
    expect(onClose).not.toHaveBeenCalled();
  });

  it("edits from the entry's values and saves under the edit title", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    render(
      <WorklogDialog
        initial={{ minutes: 30, logged_on: "2026-10-19", note: "First pass" }}
        saving={false}
        onClose={vi.fn()}
        onSubmit={onSubmit}
      />
    );

    expect(screen.getByRole("heading", { name: "Edit time entry" })).toBeDefined();
    expect((screen.getByLabelText("Minutes") as HTMLInputElement).value).toBe("30");
    expect((screen.getByLabelText("Date") as HTMLInputElement).value).toBe("2026-10-19");
    expect((screen.getByLabelText("Note") as HTMLInputElement).value).toBe("First pass");

    await user.clear(screen.getByLabelText("Minutes"));
    await user.type(screen.getByLabelText("Minutes"), "45");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(onSubmit).toHaveBeenCalledWith({ minutes: 45, logged_on: "2026-10-19", note: "First pass" });
  });

  it("explains invalid minutes and a missing day instead of submitting", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    render(<WorklogDialog saving={false} onClose={vi.fn()} onSubmit={onSubmit} />);
    const save = screen.getByRole("button", { name: "Log time" });

    await user.type(screen.getByLabelText("Minutes"), "1.5");
    await user.clear(screen.getByLabelText("Date"));
    await user.click(save);
    expect(onSubmit).not.toHaveBeenCalled();
    expect(screen.getByText("Enter whole minutes above zero.")).toBeDefined();
    expect(screen.getByText("Pick the day the work happened.")).toBeDefined();
  });

  it("holds the save when only the day is missing", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    render(<WorklogDialog saving={false} onClose={vi.fn()} onSubmit={onSubmit} />);

    await user.type(screen.getByLabelText("Minutes"), "30");
    await user.clear(screen.getByLabelText("Date"));
    await user.click(screen.getByRole("button", { name: "Log time" }));
    expect(onSubmit).not.toHaveBeenCalled();
    expect(screen.queryByText("Enter whole minutes above zero.")).toBeNull();
    expect(screen.getByText("Pick the day the work happened.")).toBeDefined();
  });

  it("cancels without submitting", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    const onClose = vi.fn();
    render(<WorklogDialog saving={true} onClose={onClose} onSubmit={onSubmit} />);

    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("dismisses with Escape without submitting", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    const onClose = vi.fn();
    render(<WorklogDialog saving={false} onClose={onClose} onSubmit={onSubmit} />);

    await user.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onSubmit).not.toHaveBeenCalled();
  });
});
