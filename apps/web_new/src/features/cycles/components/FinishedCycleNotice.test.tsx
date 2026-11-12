// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Finished-cycle read-only notice: the finished/not-editable wording
// the parity driver reads, plus the transfer slot (CYC-023).

import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { FinishedCycleNotice } from "./FinishedCycleNotice.js";

describe("FinishedCycleNotice", () => {
  it("explains the finished cycle is not editable", () => {
    render(<FinishedCycleNotice />);
    const notice = screen.getByText(/not editable/i);
    expect(notice).toHaveTextContent(/has finished/i);
    expect(screen.getByRole("region", { name: "Finished cycle" })).toContainElement(notice);
  });

  it("renders the transfer prompt below the notice", () => {
    render(
      <FinishedCycleNotice>
        <button type="button">Transfer leftovers</button>
      </FinishedCycleNotice>
    );
    const section = screen.getByRole("region", { name: "Finished cycle" });
    expect(section).toHaveTextContent("Transfer leftovers");
    expect(section.textContent?.indexOf("not editable") ?? -1).toBeLessThan(
      section.textContent?.indexOf("Transfer leftovers") ?? -1
    );
  });
});
