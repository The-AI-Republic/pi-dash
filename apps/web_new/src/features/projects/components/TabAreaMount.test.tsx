// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { TabAreaMount } from "./TabAreaMount.js";

describe("TabAreaMount", () => {
  it("names the area inside its home region", () => {
    render(<TabAreaMount area="Cycles" />);
    expect(screen.getByRole("heading", { name: "Cycles" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Cycles home" })).toBeInTheDocument();
  });
});
