// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { emptyTokenDraft, expiryLine, resolveExpiry, type TokenDraft } from "./model.js";
import { accessibleDayName, expiryMonthGrid, TokenExpiryCalendar, TokenExpiryControls } from "./TokenExpiryPicker.js";

const NOW = new Date(2026, 9, 19, 11, 30, 0, 0);

describe("expiryMonthGrid", () => {
  it("builds six weeks starting on Sunday and marks pre-floor days", () => {
    const weeks = expiryMonthGrid(2026, 9, "2026-10-20");
    expect(weeks).toHaveLength(6);
    expect(weeks.flat()).toHaveLength(42);
    expect(weeks[0]?.[0]?.iso).toBe("2026-09-27");
    const byIso = new Map(weeks.flat().map((cell) => [cell.iso, cell]));
    expect(byIso.get("2026-10-19")?.disabled).toBe(true);
    expect(byIso.get("2026-10-20")?.disabled).toBe(false);
    expect(byIso.get("2026-10-01")?.inMonth).toBe(true);
    expect(byIso.get("2026-09-27")?.inMonth).toBe(false);
  });
});

describe("accessibleDayName", () => {
  it("uses a weekday, full month and correct ordinal", () => {
    expect(accessibleDayName("2026-10-01")).toBe("Thursday, October 1st, 2026");
    expect(accessibleDayName("2026-10-22")).toBe("Thursday, October 22nd, 2026");
    expect(accessibleDayName("2026-10-23")).toBe("Friday, October 23rd, 2026");
    expect(accessibleDayName("2026-10-11")).toBe("Sunday, October 11th, 2026");
    expect(accessibleDayName("2026-10-12")).toBe("Monday, October 12th, 2026");
    expect(accessibleDayName("2026-10-13")).toBe("Tuesday, October 13th, 2026");
    expect(accessibleDayName("2026-10-24")).toBe("Saturday, October 24th, 2026");
  });
});

describe("TokenExpiryCalendar", () => {
  it("disables today, enables tomorrow, moves months, and reports a pick", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    render(<TokenExpiryCalendar value={null} minDay="2026-10-20" onSelect={onSelect} now={NOW} />);
    expect(screen.getByRole("button", { name: accessibleDayName("2026-10-19") })).toBeDisabled();
    const tomorrow = screen.getByRole("button", { name: accessibleDayName("2026-10-20") });
    expect(tomorrow).toBeEnabled();
    await user.click(tomorrow);
    expect(onSelect).toHaveBeenCalledWith("2026-10-20");

    await user.click(screen.getByRole("button", { name: "Go to the Next Month" }));
    expect(screen.getByText("November 2026")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Go to the Previous Month" }));
    expect(screen.getByText("October 2026")).toBeInTheDocument();
  });

  it("opens on the picked month and marks the selection", () => {
    render(<TokenExpiryCalendar value="2027-02-10" minDay="2026-10-20" onSelect={() => undefined} now={NOW} />);
    expect(screen.getByText("February 2027")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: accessibleDayName("2027-02-10") })).toHaveAttribute(
      "aria-pressed",
      "true"
    );
  });
});

function ControlsHarness({ initial = emptyTokenDraft() }: { initial?: TokenDraft }): React.ReactElement {
  const [draft, setDraft] = React.useState(initial);
  return <TokenExpiryControls draft={draft} onChange={setDraft} now={NOW} />;
}

describe("TokenExpiryControls", () => {
  it("shows the placeholder for an unknown preset label", () => {
    const draft = { ...emptyTokenDraft(), choice: "bogus" as never };
    render(<TokenExpiryControls draft={draft} onChange={() => undefined} now={NOW} />);
    expect(screen.getByRole("button", { name: "Set expiration" })).toBeInTheDocument();
  });

  it("lists the four presets plus Custom and previews the chosen preset", async () => {
    const user = userEvent.setup();
    render(<ControlsHarness />);
    await user.click(screen.getByRole("button", { name: "Set expiration" }));
    const options = screen.getAllByRole("option");
    expect(options.map((option) => option.textContent)).toEqual(["1 week", "1 month", "3 months", "1 year", "Custom"]);
    await user.click(screen.getByRole("option", { name: "1 week" }));
    expect(screen.queryAllByRole("option")).toHaveLength(0);
    expect(screen.getByRole("button", { name: "1 week" })).toBeInTheDocument();
    const expected = resolveExpiry({ ...emptyTokenDraft(), choice: "week" }, NOW) as string;
    expect(screen.getByText(expiryLine(expected))).toBeInTheDocument();
  });

  it("picks a custom day and previews it, then never-expires disables and hides the preview", async () => {
    const user = userEvent.setup();
    render(<ControlsHarness />);
    await user.click(screen.getByRole("button", { name: "Set expiration" }));
    await user.click(screen.getByRole("option", { name: "Custom" }));
    await user.click(screen.getByText("Set date"));
    const calendar = screen.getByTestId("token-expiry-calendar");
    await user.click(screen.getByRole("button", { name: "Go to the Next Month" }));
    await user.click(within(calendar).getByRole("button", { name: accessibleDayName("2026-11-15") }));
    expect(screen.getByText("2026-11-15")).toBeInTheDocument();
    const expected = resolveExpiry({ ...emptyTokenDraft(), choice: "custom", customDay: "2026-11-15" }, NOW) as string;
    expect(screen.getByText(expiryLine(expected))).toBeInTheDocument();

    await user.click(screen.getByRole("switch", { name: "Never expires" }));
    expect(screen.getByRole("button", { name: "Custom date" })).toBeDisabled();
    expect(screen.getByText("2026-11-15")).toBeInTheDocument();
    expect(screen.queryByText(/^Expires /)).toBeNull();

    await user.click(screen.getByRole("switch", { name: "Never expires" }));
    expect(screen.getByRole("button", { name: "Custom date" })).toBeEnabled();
    expect(screen.getByText(expiryLine(expected))).toBeInTheDocument();
  });

  it("toggles never-expires from the label as well as the switch", async () => {
    const user = userEvent.setup();
    render(<ControlsHarness initial={{ ...emptyTokenDraft(), choice: "month" }} />);
    await user.click(screen.getByRole("button", { name: "Never expires" }));
    expect(screen.getByRole("switch", { name: "Never expires" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("button", { name: "1 month" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Never expires" }));
    expect(screen.getByRole("switch", { name: "Never expires" })).toHaveAttribute("aria-checked", "false");
  });
});
