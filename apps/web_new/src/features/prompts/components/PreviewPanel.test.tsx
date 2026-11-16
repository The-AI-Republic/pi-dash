// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Render-against-real-data form (parity/specs/agents.md §7, AGT-030, AGT-031).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { ApiError } from "@pidash/api-client";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const { contractMocks } = vi.hoisted(() => ({ contractMocks: { previewPrompt: vi.fn() } }));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

import {
  MISSING_INSTALL_TARGET,
  MISSING_ISSUE_TARGET,
  PREVIEW_FAILED_FALLBACK,
  previewFailureMessage,
} from "../api/mutations.js";
import { PreviewPanel } from "./PreviewPanel.js";

const SLUG = "acme";

function renderPanel(props: Partial<React.ComponentProps<typeof PreviewPanel>> = {}) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <PreviewPanel workspaceSlug={SLUG} kind="coding-task" submitLabel="Preview" testIdPrefix="panel" {...props} />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => {
  resetAppClient();
});

describe("PreviewPanel", () => {
  it("refuses an empty target by disabling submit", () => {
    renderPanel();
    expect(screen.getByRole("button", { name: "Preview" })).toBeDisabled();
    fireEvent.change(screen.getByPlaceholderText("Issue id (UUID)"), { target: { value: "   " } });
    expect(screen.getByRole("button", { name: "Preview" })).toBeDisabled();
  });

  it("labels the target per kind and renders the heading when given", () => {
    renderPanel({ kind: "scheduler", heading: "Preview", submitLabel: "Preview draft" });
    expect(screen.getByRole("heading", { name: "Preview" })).toBeInTheDocument();
    expect(screen.getByPlaceholderText("Scheduler install id (UUID)")).toBeInTheDocument();
    expect(screen.getByText("Scheduler install id")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Preview draft" })).toBeInTheDocument();
  });

  it("renders the returned prompt text for an issue target", async () => {
    contractMocks.previewPrompt.mockResolvedValue({ kind: "coding-task", prompt: "Rendered prompt" });
    const user = userEvent.setup();
    renderPanel();
    await user.type(screen.getByRole("textbox"), "issue-1");
    await user.click(screen.getByRole("button", { name: "Preview" }));
    await waitFor(() => expect(screen.getByTestId("panel-result")).toHaveTextContent("Rendered prompt"));
    expect(contractMocks.previewPrompt).toHaveBeenCalledWith(expect.anything(), SLUG, "coding-task", {
      issue_id: "issue-1",
    });
    expect(screen.queryByTestId("panel-error")).not.toBeInTheDocument();
  });

  it("sends the install target and the draft scope for unsaved previews", async () => {
    contractMocks.previewPrompt.mockResolvedValue({ kind: "scheduler", prompt: "Draft render" });
    const user = userEvent.setup();
    renderPanel({
      kind: "scheduler",
      submitLabel: "Preview draft",
      draft: { scope: "user", sectionKey: "autonomy", body: "draft body" },
    });
    await user.type(screen.getByRole("textbox"), "binding-1");
    await user.click(screen.getByRole("button", { name: "Preview draft" }));
    await waitFor(() =>
      expect(contractMocks.previewPrompt).toHaveBeenCalledWith(expect.anything(), SLUG, "scheduler", {
        binding_id: "binding-1",
        scope: "user",
        section_key: "autonomy",
        body: "draft body",
      })
    );
  });

  it("lands the failure inline and clears a previous render", async () => {
    contractMocks.previewPrompt
      .mockResolvedValueOnce({ kind: "coding-task", prompt: "First render" })
      .mockRejectedValueOnce(new Error("parity preview failure"));
    const user = userEvent.setup();
    renderPanel();
    await user.type(screen.getByRole("textbox"), "issue-1");
    await user.click(screen.getByRole("button", { name: "Preview" }));
    await waitFor(() => expect(screen.getByTestId("panel-result")).toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: "Preview" }));
    await waitFor(() => expect(screen.getByTestId("panel-error")).toHaveTextContent("parity preview failure"));
    expect(screen.queryByTestId("panel-result")).not.toBeInTheDocument();
  });

  it("refuses a blank target submitted through the form with the kind's copy", () => {
    renderPanel();
    fireEvent.submit(screen.getByRole("textbox").closest("form") as HTMLFormElement);
    expect(screen.getByTestId("panel-error")).toHaveTextContent(MISSING_ISSUE_TARGET);
    expect(contractMocks.previewPrompt).not.toHaveBeenCalled();
  });

  it("uses the install copy for a blank scheduler target", () => {
    renderPanel({
      kind: "scheduler",
      submitLabel: "Preview draft",
      draft: { scope: "user", sectionKey: "k", body: "b" },
    });
    fireEvent.submit(screen.getByRole("textbox").closest("form") as HTMLFormElement);
    expect(screen.getByTestId("panel-error")).toHaveTextContent(MISSING_INSTALL_TARGET);
  });

  it("does not resubmit while a preview is in flight", async () => {
    let settle: (value: { kind: string; prompt: string }) => void = () => undefined;
    contractMocks.previewPrompt.mockImplementation(
      () =>
        new Promise((resolve) => {
          settle = resolve;
        })
    );
    const user = userEvent.setup();
    renderPanel();
    await user.type(screen.getByRole("textbox"), "issue-1");
    await user.click(screen.getByRole("button", { name: "Preview" }));
    await waitFor(() => expect(contractMocks.previewPrompt).toHaveBeenCalledTimes(1));
    fireEvent.submit(screen.getByRole("textbox").closest("form") as HTMLFormElement);
    expect(contractMocks.previewPrompt).toHaveBeenCalledTimes(1);
    settle({ kind: "coding-task", prompt: "Done" });
    await waitFor(() => expect(screen.getByTestId("panel-result")).toHaveTextContent("Done"));
  });
});

describe("previewFailureMessage", () => {
  it("prefers the first field message, then the normalized message", () => {
    expect(
      previewFailureMessage(
        new ApiError({ status: 422, code: "http", message: "422", fields: { target: ["Target not found."] } })
      )
    ).toBe("Target not found.");
    expect(previewFailureMessage(new Error("Render failed upstream."))).toBe("Render failed upstream.");
    expect(previewFailureMessage(new ApiError({ status: 500, code: "http", message: "Server exploded." }))).toBe(
      "Server exploded."
    );
  });

  it("falls back for empty, non-error and unknown failures", () => {
    expect(previewFailureMessage(new Error(""))).toBe(PREVIEW_FAILED_FALLBACK);
    expect(previewFailureMessage("boom")).toBe(PREVIEW_FAILED_FALLBACK);
    expect(previewFailureMessage({ message: "" })).toBe(PREVIEW_FAILED_FALLBACK);
    expect(
      previewFailureMessage(new ApiError({ status: 422, code: "http", message: "422", fields: { target: [] } }))
    ).toBe("422");
    expect(
      previewFailureMessage(new ApiError({ status: 422, code: "http", message: "", fields: { target: [] } }))
    ).toBe(PREVIEW_FAILED_FALLBACK);
  });
});
