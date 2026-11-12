// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pod filter affordance (NEWFRONT-540 groundwork, RUN-012). The bar names
// the active pod and clears it; the tiles live in the pods section.
import type { Pod } from "@pidash/api-client";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { PodFilterBar } from "./RunnersPodFilter.js";

const pod = (id: string, overrides: Partial<Pod> = {}): Pod =>
  ({ id, name: id, is_default: false, runner_count: 0, ...overrides }) as Pod;

describe("PodFilterBar", () => {
  it("names the selected pod and clears through its button", async () => {
    const onClear = vi.fn();
    render(<PodFilterBar selectedPod={pod("p-1", { name: "Alpha" })} onClear={onClear} />);
    expect(screen.getByText("Filtering runners by pod Alpha")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Clear filter" }));
    expect(onClear).toHaveBeenCalledTimes(1);
  });
});
