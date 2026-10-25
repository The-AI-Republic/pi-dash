// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Create dialog (parity/specs/pages.md §3.6 and §11.3; PAGE-008): a required
// trimmed title, an inline block past 255 characters, an optional mark and
// an open-or-restricted choice. Cancel and Escape write nothing; a failed
// write keeps the dialog and its draft; a successful one closes and hands
// the new id back.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const contracts = vi.hoisted(() => ({ createPage: vi.fn() }));
const shell = vi.hoisted(() => ({ notifyError: vi.fn() }));

vi.mock("@pidash/api-client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pidash/api-client")>()),
  createPage: contracts.createPage,
}));
vi.mock("../../../shared/shell/toaster.js", () => ({
  notifyPending: vi.fn(),
  notifySuccess: vi.fn(),
  notifyError: shell.notifyError,
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { PAGES_PAGE_ID, PAGES_PROJECT_ID, makePageRow } from "../testing.js";
import { PAGE_CREATE_DIALOG_TITLE, PAGE_CREATE_SUBMIT_LABEL, PageCreateDialog } from "./PageCreateDialog.js";

const TITLE_INPUT = "Title";

interface Harness {
  onOpenChange: ReturnType<typeof vi.fn>;
  onCreated: ReturnType<typeof vi.fn>;
}

function renderDialog(overrides: { initialAccess?: number; open?: boolean; onCreated?: boolean } = {}): Harness {
  const onOpenChange = vi.fn();
  const onCreated = vi.fn();
  const props = {
    open: overrides.open ?? true,
    onOpenChange,
    workspaceSlug: "acme",
    projectId: PAGES_PROJECT_ID,
    ...(overrides.initialAccess === undefined ? {} : { initialAccess: overrides.initialAccess }),
    ...(overrides.onCreated === false ? {} : { onCreated }),
  };
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { mutations: { retry: false } } })}>
      <PageCreateDialog {...props} />
    </QueryClientProvider>
  );
  return { onOpenChange, onCreated };
}

function titleField(): HTMLElement {
  return screen.getByPlaceholderText(TITLE_INPUT);
}

function submitButton(): HTMLElement {
  return screen.getByRole("button", { name: PAGE_CREATE_SUBMIT_LABEL });
}

beforeEach(() => {
  vi.clearAllMocks();
  contracts.createPage.mockResolvedValue({ ...makePageRow({ id: PAGES_PAGE_ID }), description_html: "" });
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => {
  resetAppClient();
});

describe("PageCreateDialog", () => {
  it("blocks an empty title, allows a filled one and starts open by default", () => {
    renderDialog();
    expect(screen.getByRole("dialog", { name: PAGE_CREATE_DIALOG_TITLE })).toBeInTheDocument();
    expect(PAGE_CREATE_DIALOG_TITLE).toBe("Create page");
    expect(submitButton()).toBeDisabled();
    expect(titleField()).toBeRequired();
    expect(screen.getByTestId("page-create-access-label")).toHaveTextContent("Public");

    fireEvent.change(titleField(), { target: { value: "   " } });
    expect(submitButton()).toBeDisabled();

    fireEvent.change(titleField(), { target: { value: "Launch notes" } });
    expect(submitButton()).toBeEnabled();
  });

  it("shows the inline over-length block and refuses to submit past 255 characters", () => {
    renderDialog();
    fireEvent.change(titleField(), { target: { value: "d".repeat(256) } });
    expect(screen.getByText(/Keep the title to 255 characters/)).toBeInTheDocument();
    expect(titleField()).toHaveAttribute("aria-invalid", "true");
    expect(submitButton()).toBeDisabled();

    fireEvent.submit(submitButton().closest("form") as HTMLFormElement);
    expect(contracts.createPage).not.toHaveBeenCalled();

    fireEvent.change(titleField(), { target: { value: "d".repeat(255) } });
    expect(screen.queryByText(/Keep the title to 255 characters/)).not.toBeInTheDocument();
    expect(submitButton()).toBeEnabled();
  });

  it("starts on the tab's visibility and lets the operator switch it", () => {
    renderDialog({ initialAccess: 1 });
    expect(screen.getByTestId("page-create-access-label")).toHaveTextContent("Private");
    expect(screen.getByRole("button", { name: "Private" })).toHaveAttribute("aria-pressed", "true");

    fireEvent.click(screen.getByRole("button", { name: "Public" }));
    expect(screen.getByTestId("page-create-access-label")).toHaveTextContent("Public");
    expect(screen.getByRole("button", { name: "Private" })).toHaveAttribute("aria-pressed", "false");
  });

  it("picks a mark and posts it with the trimmed title and chosen visibility", async () => {
    const { onOpenChange, onCreated } = renderDialog();
    fireEvent.change(titleField(), { target: { value: "  Launch notes  " } });
    fireEvent.click(screen.getByRole("button", { name: "Private" }));

    fireEvent.click(screen.getByTestId("page-create-mark"));
    fireEvent.click(await screen.findByRole("tab", { name: "Emoji" }));
    fireEvent.change(await screen.findByPlaceholderText("Search or type an emoji"), { target: { value: "fire" } });
    fireEvent.click(await screen.findByRole("button", { name: "fire" }));

    fireEvent.click(submitButton());

    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
    expect(contracts.createPage).toHaveBeenCalledWith(expect.anything(), "acme", PAGES_PROJECT_ID, {
      name: "Launch notes",
      access: 1,
      logo_props: { in_use: "emoji", emoji: { value: "128293" } },
    });
    expect(onCreated).toHaveBeenCalledWith(PAGES_PAGE_ID);
  });

  it("closes without creating when cancelled, and clears the draft for the next open", () => {
    const { onOpenChange, onCreated } = renderDialog();
    fireEvent.change(titleField(), { target: { value: "Discard me" } });
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(contracts.createPage).not.toHaveBeenCalled();
    expect(onCreated).not.toHaveBeenCalled();
  });

  it("shows nothing while closed and resets the draft on a fresh open", () => {
    const client = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
    const onOpenChange = vi.fn();
    const props = {
      onOpenChange,
      workspaceSlug: "acme",
      projectId: PAGES_PROJECT_ID,
      initialAccess: 0,
    };
    const view = render(
      <QueryClientProvider client={client}>
        <PageCreateDialog {...props} open={false} />
      </QueryClientProvider>
    );
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    view.rerender(
      <QueryClientProvider client={client}>
        <PageCreateDialog {...props} open />
      </QueryClientProvider>
    );
    fireEvent.change(titleField(), { target: { value: "Draft" } });
    expect(titleField()).toHaveValue("Draft");

    view.rerender(
      <QueryClientProvider client={client}>
        <PageCreateDialog {...props} open={false} />
      </QueryClientProvider>
    );
    view.rerender(
      <QueryClientProvider client={client}>
        <PageCreateDialog {...props} open />
      </QueryClientProvider>
    );
    expect(titleField()).toHaveValue("");
  });

  it("keeps the dialog and its draft when the write fails", async () => {
    contracts.createPage.mockRejectedValue(new Error("nope"));
    const { onOpenChange, onCreated } = renderDialog();
    fireEvent.change(titleField(), { target: { value: "Retry me" } });

    fireEvent.click(submitButton());

    await waitFor(() => expect(shell.notifyError).toHaveBeenCalledWith("Page could not be created."));
    expect(titleField()).toHaveValue("Retry me");
    expect(screen.getByRole("dialog", { name: PAGE_CREATE_DIALOG_TITLE })).toBeInTheDocument();
    expect(onOpenChange).not.toHaveBeenCalledWith(false);
    expect(onCreated).not.toHaveBeenCalled();
  });

  it("closes and still works when the caller asks for no redirect", async () => {
    const { onOpenChange } = renderDialog({ onCreated: false });
    fireEvent.change(titleField(), { target: { value: "Stay here" } });
    fireEvent.click(submitButton());
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
    expect(contracts.createPage).toHaveBeenCalledTimes(1);
  });

  it("dismisses on Escape and discards the draft", () => {
    const { onOpenChange } = renderDialog();
    fireEvent.change(titleField(), { target: { value: "Escape me" } });
    fireEvent.keyDown(screen.getByRole("dialog", { name: PAGE_CREATE_DIALOG_TITLE }), {
      key: "Escape",
      code: "Escape",
    });
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(contracts.createPage).not.toHaveBeenCalled();
  });

  it("busies the submit and ignores dismissal while the write flies", async () => {
    contracts.createPage.mockImplementation(() => new Promise<never>(() => undefined));
    const { onOpenChange } = renderDialog();
    fireEvent.change(titleField(), { target: { value: "Slow page" } });
    fireEvent.click(submitButton());

    await waitFor(() => expect(submitButton()).toBeDisabled());
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).not.toHaveBeenCalledWith(false);
    expect(screen.getByRole("dialog", { name: PAGE_CREATE_DIALOG_TITLE })).toBeInTheDocument();
  });
});
