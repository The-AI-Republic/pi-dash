// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { pickerKeys } from "../../../shared/pickers/reference.js";

const mocks = vi.hoisted(() => ({
  cycles: vi.fn(),
  transfer: vi.fn(),
  notifySuccess: vi.fn(),
  notifyError: vi.fn(),
}));

vi.mock("../../../shared/pickers/reference.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../shared/pickers/reference.js")>()),
  usePickerCycles: mocks.cycles,
}));
vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, transferCycleIssues: mocks.transfer };
});
vi.mock("../../../shared/shell/index.js", () => ({
  notifySuccess: mocks.notifySuccess,
  notifyError: mocks.notifyError,
}));

import { TRANSFER_FAILED_BODY, TRANSFER_FAILED_TITLE, TRANSFER_SUCCESS_BODY, TRANSFER_SUCCESS_TITLE } from "./api.js";
import { TransferIssuesDialog, TRANSFER_SEARCH_PLACEHOLDER } from "./TransferIssuesDialog.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const SOURCE = "323e4567-e89b-12d3-a456-426614174002";
const TARGET = "423e4567-e89b-12d3-a456-426614174003";

function cycles(): unknown[] {
  return [
    { id: SOURCE, project_id: PROJECT, name: "Source cycle", is_favorite: false, status: "COMPLETED" },
    { id: TARGET, project_id: PROJECT, name: "Target cycle", is_favorite: false, status: "ACTIVE" },
    { id: "draft", project_id: PROJECT, name: "Draft cycle", is_favorite: false, status: "DRAFT" },
  ];
}

function setup(open = true) {
  const onClose = vi.fn();
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  render(
    <QueryClientProvider client={queryClient}>
      <TransferIssuesDialog workspaceSlug={SLUG} projectId={PROJECT} cycleId={SOURCE} open={open} onClose={onClose} />
    </QueryClientProvider>
  );
  return { onClose, queryClient };
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
  mocks.cycles.mockReturnValue({ data: cycles(), isPending: false });
  mocks.transfer.mockResolvedValue({ message: "ok" });
});

afterEach(() => {
  resetAppClient();
});

describe("TransferIssuesDialog", () => {
  it("lists incomplete cycles except the source under the search placeholder", () => {
    setup();
    expect(screen.getByPlaceholderText(TRANSFER_SEARCH_PLACEHOLDER)).toBeInTheDocument();
    expect(screen.getByText("Target cycle")).toBeInTheDocument();
    expect(screen.getByText("Draft cycle")).toBeInTheDocument();
    expect(screen.queryByText("Source cycle")).not.toBeInTheDocument();
    expect(screen.getByText("active")).toBeInTheDocument();
  });

  it("narrows the list as the reader types", async () => {
    const user = userEvent.setup();
    setup();
    await user.type(screen.getByPlaceholderText(TRANSFER_SEARCH_PLACEHOLDER), "draft");
    expect(screen.queryByText("Target cycle")).not.toBeInTheDocument();
    expect(screen.getByText("Draft cycle")).toBeInTheDocument();
  });

  it("transfers to the picked cycle, toasts and closes", async () => {
    const user = userEvent.setup();
    const { onClose, queryClient } = setup();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    await user.click(screen.getByRole("option", { name: /Target cycle/ }));
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(mocks.transfer).toHaveBeenCalledWith(expect.anything(), SLUG, PROJECT, SOURCE, TARGET);
    expect(mocks.notifySuccess).toHaveBeenCalledWith(TRANSFER_SUCCESS_TITLE, TRANSFER_SUCCESS_BODY);
    await waitFor(() =>
      expect(invalidate).toHaveBeenCalledWith(expect.objectContaining({ queryKey: pickerKeys.cycles(SLUG, PROJECT) }))
    );
  });

  it("keeps the dialog open and notices when the transfer is refused", async () => {
    const user = userEvent.setup();
    mocks.transfer.mockRejectedValue(new Error("nope"));
    const { onClose } = setup();
    await user.click(screen.getByRole("option", { name: /Target cycle/ }));
    await waitFor(() => expect(mocks.notifyError).toHaveBeenCalledWith(TRANSFER_FAILED_TITLE, TRANSFER_FAILED_BODY));
    expect(onClose).not.toHaveBeenCalled();
  });

  it("explains when no incomplete cycle can take the work", () => {
    mocks.cycles.mockReturnValue({ data: cycles().slice(0, 1), isPending: false });
    setup();
    expect(screen.getByText("No incomplete cycle to move the work into.")).toBeInTheDocument();
  });

  it("closes when the reader dismisses the dialog", async () => {
    const user = userEvent.setup();
    const { onClose } = setup();
    await user.click(screen.getByRole("button", { name: "Close dialog" }));
    expect(onClose).toHaveBeenCalled();
  });

  it("closes on Escape and leaves the source out of the list", async () => {
    const user = userEvent.setup();
    const { onClose } = setup();
    await user.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalled();
  });

  it("omits the status pill for a cycle the server sent without one", () => {
    mocks.cycles.mockReturnValue({
      data: [{ id: TARGET, project_id: PROJECT, name: "No status cycle", is_favorite: false }],
      isPending: false,
    });
    setup();
    expect(screen.getByText("No status cycle")).toBeInTheDocument();
    expect(screen.queryByText("active")).not.toBeInTheDocument();
  });

  it("shows a loading line before the cycle list resolves", () => {
    mocks.cycles.mockReturnValue({ data: undefined, isPending: true });
    setup();
    expect(screen.getByText("Loading cycles…")).toBeInTheDocument();
  });
});
