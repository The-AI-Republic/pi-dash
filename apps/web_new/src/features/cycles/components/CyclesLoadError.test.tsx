// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The load error retries the failed read.

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { CyclesLoadError } from "./CyclesLoadError.js";

describe("CyclesLoadError", () => {
  it("retries on select", () => {
    const onRetry = vi.fn();
    render(<CyclesLoadError onRetry={onRetry} />);
    expect(screen.getByTestId("cycles-load-error")).toHaveTextContent(/didn't load/);
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalledTimes(1);
  });
});
