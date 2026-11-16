// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

const useRunnerDetail = vi.fn();
vi.mock("../api/detail.js", () => ({
  useRunnerDetail: (slug: string, id: string) => useRunnerDetail(slug, id),
}));
vi.mock("./RunnerDetailScreen.js", () => ({
  RunnerDetailScreen: (props: Record<string, unknown>) => (
    <div data-testid="detail-screen">{JSON.stringify(props)}</div>
  ),
}));
vi.mock("@tanstack/react-router", () => ({ Link: ({ children }: { children: React.ReactNode }) => <>{children}</> }));

import { RunnerDetailContainer } from "./RunnerDetailContainer.js";

describe("RunnerDetailContainer", () => {
  it("passes the read through to the screen", () => {
    useRunnerDetail.mockReturnValue({ data: { id: "runner-1" }, isPending: false, isError: false });
    render(<RunnerDetailContainer workspaceSlug="acme" runnerId="runner-1" projectId="prj-1" />);
    expect(useRunnerDetail).toHaveBeenCalledWith("acme", "runner-1");
    const props = JSON.parse(screen.getByTestId("detail-screen").textContent ?? "{}");
    expect(props).toMatchObject({
      workspaceSlug: "acme",
      runnerId: "runner-1",
      projectId: "prj-1",
      loading: false,
      failed: false,
    });
  });

  it("reports a failure only when no data is cached", () => {
    useRunnerDetail.mockReturnValue({ data: { id: "runner-1" }, isPending: false, isError: true });
    render(<RunnerDetailContainer workspaceSlug="acme" runnerId="runner-1" />);
    let props = JSON.parse(screen.getByTestId("detail-screen").textContent ?? "{}");
    expect(props.failed).toBe(false);

    useRunnerDetail.mockReturnValue({ data: undefined, isPending: false, isError: true });
    render(<RunnerDetailContainer workspaceSlug="acme" runnerId="runner-2" />);
    props = JSON.parse(screen.getAllByTestId("detail-screen").at(-1)?.textContent ?? "{}");
    expect(props.failed).toBe(true);
  });
});
