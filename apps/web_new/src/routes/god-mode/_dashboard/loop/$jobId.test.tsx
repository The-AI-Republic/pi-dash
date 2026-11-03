// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// What this suite owns is the mount: the detail route hands the
// path's job id to the job page. Detail behavior belongs to the
// admin area.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("../../../../features/admin/components/AdminLoopJobDetailPage.js", () => ({
  AdminLoopJobDetailPage: ({ jobId }: { jobId: string }) => <div data-testid="loop-job-detail-page">{jobId}</div>,
}));

import { Route } from "./$jobId.js";

describe("loop job detail route", () => {
  it("mounts the detail page with the path job id", () => {
    const Component = Route.options.component;
    if (!Component) throw new Error("loop job detail route has no component");
    vi.spyOn(Route, "useParams").mockReturnValue({ jobId: "job-1" });
    render(<Component />);
    expect(screen.getByTestId("loop-job-detail-page")).toHaveTextContent("job-1");
  });
});
