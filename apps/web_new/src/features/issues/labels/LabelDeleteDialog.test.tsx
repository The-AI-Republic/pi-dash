// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ApiError, type Label } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const { contractMocks } = vi.hoisted(() => ({ contractMocks: { deleteLabel: vi.fn() } }));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
}));

import { notifyError } from "../../../shared/shell/index.js";
import { LABEL_DELETE_FAILED_TITLE } from "./api.js";
import { LabelDeleteDialog } from "./LabelDeleteDialog.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";

function label(): Label {
  return {
    id: "a",
    project_id: PROJECT,
    workspace_id: "323e4567-e89b-12d3-a456-426614174002",
    name: "Doomed",
    color: "#EB144C",
    parent: null,
    sort_order: 10000,
  };
}

function setup(target: Label | null) {
  const onClose = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <LabelDeleteDialog workspaceSlug={SLUG} projectId={PROJECT} label={target} onClose={onClose} />
    </QueryClientProvider>
  );
  return { onClose };
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => {
  resetAppClient();
});

describe("LabelDeleteDialog", () => {
  it("renders nothing without a target", () => {
    setup(null);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("names the label and warns about removal", () => {
    setup(label());
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveTextContent("Doomed");
    expect(dialog).toHaveTextContent("remove the label from all the work items");
  });

  it("closes on cancel without deleting", async () => {
    const user = userEvent.setup();
    const { onClose } = setup(label());
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(contractMocks.deleteLabel).not.toHaveBeenCalled();
  });

  it("deletes on confirm and closes", async () => {
    const user = userEvent.setup();
    contractMocks.deleteLabel.mockResolvedValue(undefined);
    const { onClose } = setup(label());
    await user.click(screen.getByRole("button", { name: "Delete label" }));
    await waitFor(() => expect(contractMocks.deleteLabel).toHaveBeenCalledTimes(1));
    expect(contractMocks.deleteLabel).toHaveBeenCalledWith(expect.anything(), SLUG, PROJECT, "a");
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("locks dismissal while deleting, then stays open with a notice on failure", async () => {
    const user = userEvent.setup();
    let reject!: (error: unknown) => void;
    contractMocks.deleteLabel.mockReturnValue(
      new Promise<never>((_resolve, rejectPromise) => (reject = rejectPromise))
    );
    const { onClose } = setup(label());
    expect(screen.getByRole("button", { name: "Close dialog" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Delete label" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Deleting…" })).toBeDisabled());
    expect(screen.queryByRole("button", { name: "Close dialog" })).not.toBeInTheDocument();
    reject(new ApiError({ status: 500, code: "http", message: "Down" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(LABEL_DELETE_FAILED_TITLE));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
  });
});
