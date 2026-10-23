// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { MISSING_VIEW_BACK_LABEL, MISSING_VIEW_DESCRIPTION, MISSING_VIEW_TITLE } from "./model.js";
import { SavedViewMissingState } from "./SavedViewMissingState.js";

describe("SavedViewMissingState", () => {
  it("explains the missing view and offers the way back", () => {
    const onBack = vi.fn();
    render(<SavedViewMissingState onBack={onBack} />);
    expect(screen.getByRole("heading", { name: MISSING_VIEW_TITLE })).toBeInTheDocument();
    expect(screen.getByText(MISSING_VIEW_DESCRIPTION)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: MISSING_VIEW_BACK_LABEL }));
    expect(onBack).toHaveBeenCalledTimes(1);
  });

  it("names the back control in a way the oracle's views|back|list matcher finds", () => {
    render(<SavedViewMissingState onBack={vi.fn()} />);
    expect(MISSING_VIEW_BACK_LABEL).toMatch(/views|back|list/i);
  });
});
