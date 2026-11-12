// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pending-approval queue list (NEWFRONT-547, RUN-023). Cards render in
// server order, the empty marker exists only after loading, and the
// pending id loads exactly the acted card.
import { fireEvent, render, screen } from "@testing-library/react";
import type { ApprovalRequest } from "@pidash/api-client";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { APPROVALS_EMPTY_TEXT, ApprovalsQueue } from "./ApprovalsQueue.js";

const approval = (overrides: Partial<ApprovalRequest> = {}): ApprovalRequest =>
  ({
    id: "ap-1",
    agent_run: "run-9",
    kind: "network_access",
    payload: { url: "https://example.test" },
    reason: "",
    status: "pending",
    decision_source: "",
    requested_at: "2026-10-22T10:00:00.000Z",
    decided_at: null,
    expires_at: null,
    ...overrides,
  }) as ApprovalRequest;

describe("ApprovalsQueue", () => {
  it("stacks the cards in server order", () => {
    const onDecide = vi.fn();
    render(
      <ApprovalsQueue
        approvals={[approval({ id: "ap-1" }), approval({ id: "ap-2", kind: "file_change" })]}
        loading={false}
        pendingId={null}
        onDecide={onDecide}
      />
    );
    expect(screen.getByRole("heading", { name: "Approvals" })).toBeInTheDocument();
    const cards = screen.getAllByTestId("approval-card");
    expect(cards.map((card) => card.getAttribute("data-approval-id"))).toEqual(["ap-1", "ap-2"]);
    expect(screen.queryByTestId("approvals-empty")).toBeNull();
    fireEvent.click(screen.getAllByTestId("approval-deny")[0] as HTMLElement);
    expect(onDecide).toHaveBeenCalledWith("ap-1", "decline");
  });

  it("marks only the acted card pending", () => {
    render(
      <ApprovalsQueue
        approvals={[approval({ id: "ap-1" }), approval({ id: "ap-2" })]}
        loading={false}
        pendingId="ap-2"
        onDecide={() => undefined}
      />
    );
    const denies = screen.getAllByTestId("approval-deny");
    expect(denies[0]).not.toBeDisabled();
    expect(denies[1]).toBeDisabled();
  });

  it("shows the empty marker once loading finishes with no rows", () => {
    render(<ApprovalsQueue approvals={[]} loading={false} pendingId={null} onDecide={() => undefined} />);
    expect(screen.getByTestId("approvals-empty")).toHaveTextContent(APPROVALS_EMPTY_TEXT);
    expect(screen.queryByTestId("approval-card")).toBeNull();
  });

  it("treats missing data as empty once settled", () => {
    render(<ApprovalsQueue approvals={undefined} loading={false} pendingId={null} onDecide={() => undefined} />);
    expect(screen.getByTestId("approvals-empty")).toBeInTheDocument();
  });

  it("spins without the empty marker while loading", () => {
    const { container } = render(
      <ApprovalsQueue approvals={undefined} loading={true} pendingId={null} onDecide={() => undefined} />
    );
    expect(screen.queryByTestId("approvals-empty")).toBeNull();
    expect(screen.queryByTestId("approval-card")).toBeNull();
    expect(container.querySelector('[aria-label="Approvals"]')).toBeInTheDocument();
  });
});
