// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pod filter affordances (NEWFRONT-540 groundwork, RUN-012). Tiles toggle
// the client-side filter; the bar names the active pod and clears it.
import type { Pod } from "@pidash/api-client";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { PodFilterBar, PodFilterTiles } from "./RunnersPodFilter.js";

const pod = (id: string, overrides: Partial<Pod> = {}): Pod =>
  ({ id, name: id, is_default: false, runner_count: 0, ...overrides }) as Pod;

describe("PodFilterTiles", () => {
  it("renders one labelled tile per pod with its marker and count", async () => {
    const onToggle = vi.fn();
    render(
      <PodFilterTiles
        pods={[pod("p-1", { name: "Alpha", is_default: true, runner_count: 2 }), pod("p-2", { name: "Beta" })]}
        selectedPodId="p-2"
        onToggle={onToggle}
      />
    );
    const alpha = screen.getByRole("button", { name: "Filter runners by pod Alpha" });
    const beta = screen.getByRole("button", { name: "Filter runners by pod Beta" });
    expect(alpha).toHaveTextContent("default · 2 runner(s)");
    expect(beta).toHaveTextContent("0 runner(s)");
    expect(alpha).toHaveAttribute("aria-pressed", "false");
    expect(beta).toHaveAttribute("aria-pressed", "true");
    await userEvent.click(alpha);
    expect(onToggle).toHaveBeenCalledWith("p-1");
  });
});

describe("PodFilterBar", () => {
  it("names the selected pod and clears through its button", async () => {
    const onClear = vi.fn();
    render(<PodFilterBar selectedPod={pod("p-1", { name: "Alpha" })} onClear={onClear} />);
    expect(screen.getByText("Filtering runners by pod Alpha")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Clear filter" }));
    expect(onClear).toHaveBeenCalledTimes(1);
  });
});
