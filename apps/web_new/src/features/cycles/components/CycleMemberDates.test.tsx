// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The cycle detail page shows the stored start/end dates rendered in
// the signed-in member's timezone (NEWFRONT-859): a pair joins, a lone
// date stands alone, and nothing renders without a stored date.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

const harness = vi.hoisted(() => ({
  cycleData: undefined as { start_date: string | null; end_date: string | null } | undefined,
  meData: undefined as { user_timezone: string } | undefined,
}));

vi.mock("../api/cycleDetail.js", () => ({
  useCycleDetail: () => ({ data: harness.cycleData }),
}));

vi.mock("../../../core/session/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../core/session/index.js")>()),
  useMe: () => ({ data: harness.meData }),
}));

import { CYCLE_MEMBER_DATES_LABEL, CycleMemberDates, CycleMemberDatesFrame } from "./CycleMemberDates.js";

const START = "2026-11-19T00:00:00.000Z";
const END = "2026-11-26T00:00:00.000Z";

function renderConnected() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <CycleMemberDates workspaceSlug="acme" projectId="project-1" cycleId="cycle-1" />
    </QueryClientProvider>
  );
}

describe("CycleMemberDatesFrame", () => {
  it("renders nothing without a stored date", () => {
    render(<CycleMemberDatesFrame range={{ startDate: null, endDate: null }} timeZone="UTC" />);
    expect(screen.queryByTestId("cycle-member-dates")).toBeNull();
  });

  it("renders the stored pair joined in the member timezone", () => {
    render(<CycleMemberDatesFrame range={{ startDate: START, endDate: END }} timeZone="Pacific/Kiritimati" />);
    const region = screen.getByTestId("cycle-member-dates");
    expect(region).toHaveAttribute("aria-label", CYCLE_MEMBER_DATES_LABEL);
    expect(region).toHaveTextContent(CYCLE_MEMBER_DATES_LABEL);
    expect(screen.getByTestId("cycle-member-start-date")).toHaveTextContent("Nov 19, 2026");
    expect(screen.getByTestId("cycle-member-end-date")).toHaveTextContent("Nov 26, 2026");
    expect(region).toHaveTextContent("–");
  });

  it("shifts both readings when the member timezone changes", () => {
    const { unmount } = render(
      <CycleMemberDatesFrame range={{ startDate: START, endDate: END }} timeZone="Pacific/Midway" />
    );
    expect(screen.getByTestId("cycle-member-start-date")).toHaveTextContent("Nov 18, 2026");
    expect(screen.getByTestId("cycle-member-end-date")).toHaveTextContent("Nov 25, 2026");
    unmount();
  });

  it("renders a lone start date without a joiner", () => {
    render(<CycleMemberDatesFrame range={{ startDate: START, endDate: null }} timeZone="UTC" />);
    expect(screen.getByTestId("cycle-member-start-date")).toHaveTextContent("Nov 19, 2026");
    expect(screen.queryByTestId("cycle-member-end-date")).toBeNull();
    expect(screen.getByTestId("cycle-member-dates")).not.toHaveTextContent("–");
  });

  it("renders a lone end date without a joiner", () => {
    render(<CycleMemberDatesFrame range={{ startDate: null, endDate: END }} timeZone="UTC" />);
    expect(screen.queryByTestId("cycle-member-start-date")).toBeNull();
    expect(screen.getByTestId("cycle-member-end-date")).toHaveTextContent("Nov 26, 2026");
    expect(screen.getByTestId("cycle-member-dates")).not.toHaveTextContent("–");
  });
});

describe("CycleMemberDates", () => {
  it("renders nothing while the detail read is pending", () => {
    harness.cycleData = undefined;
    harness.meData = { user_timezone: "UTC" };
    renderConnected();
    expect(screen.queryByTestId("cycle-member-dates")).toBeNull();
  });

  it("renders nothing when neither query has settled", () => {
    harness.cycleData = undefined;
    harness.meData = undefined;
    renderConnected();
    expect(screen.queryByTestId("cycle-member-dates")).toBeNull();
  });

  it("renders the stored pair from the detail and session caches", () => {
    harness.cycleData = { start_date: START, end_date: END };
    harness.meData = { user_timezone: "Pacific/Kiritimati" };
    renderConnected();
    expect(screen.getByTestId("cycle-member-start-date")).toHaveTextContent("Nov 19, 2026");
    expect(screen.getByTestId("cycle-member-end-date")).toHaveTextContent("Nov 26, 2026");
  });

  it("renders in the system zone when the member zone is missing", () => {
    harness.cycleData = { start_date: START, end_date: END };
    harness.meData = undefined;
    renderConnected();
    const local = (stored: string): string =>
      new Intl.DateTimeFormat("en-US", { month: "short", day: "2-digit", year: "numeric" }).format(new Date(stored));
    expect(screen.getByTestId("cycle-member-start-date")).toHaveTextContent(local(START));
    expect(screen.getByTestId("cycle-member-end-date")).toHaveTextContent(local(END));
  });

  it("renders nothing when the stored row carries no dates", () => {
    harness.cycleData = { start_date: null, end_date: null };
    harness.meData = { user_timezone: "UTC" };
    renderConnected();
    expect(screen.queryByTestId("cycle-member-dates")).toBeNull();
  });
});
