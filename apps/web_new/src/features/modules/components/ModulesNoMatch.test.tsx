// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The modules no-match container (MOD-010): a search-themed section state
// with a heading and a hint that tells the viewer to adjust the search or
// filters.

import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { MODULES_NO_MATCH_HEADING, MODULES_NO_MATCH_HINT, ModulesNoMatch } from "./ModulesNoMatch.js";

describe("ModulesNoMatch", () => {
  it("renders the heading and an adjustable hint", () => {
    render(<ModulesNoMatch />);
    const root = screen.getByTestId("modules-no-match");
    expect(root).toHaveTextContent(MODULES_NO_MATCH_HEADING);
    expect(root).toHaveTextContent(MODULES_NO_MATCH_HINT);
    expect(MODULES_NO_MATCH_HINT).toMatch(/adjust/i);
  });
});
