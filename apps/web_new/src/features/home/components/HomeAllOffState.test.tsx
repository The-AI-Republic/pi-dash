// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { HomeAllOffState } from "./HomeAllOffState.js";

describe("HomeAllOffState", () => {
  it("guides back to the first widget", () => {
    render(<HomeAllOffState />);
    expect(screen.getByTestId("home-all-off")).toHaveTextContent("Your dashboard is empty");
    expect(screen.getByTestId("home-all-off")).toHaveTextContent(/turn on a widget/i);
  });
});
