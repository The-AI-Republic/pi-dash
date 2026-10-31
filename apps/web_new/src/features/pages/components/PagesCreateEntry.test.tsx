// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Creation entry points (parity/specs/pages.md §3.6; PAGE-007/008): the
// header's no-form action, and the palette command that opens the dialog
// with the tab's visibility. Guests get neither.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const contracts = vi.hoisted(() => ({ createPage: vi.fn() }));

vi.mock("@pidash/api-client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pidash/api-client")>()),
  createPage: contracts.createPage,
}));

vi.mock("../../../shared/shell/toaster.js", () => ({
  notifyPending: vi.fn(),
  notifySuccess: vi.fn(),
  notifyError: vi.fn(),
}));

import { clearCommands, findCommand } from "../../../shared/commands/index.js";
import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { PAGES_PAGE_ID, PAGES_PROJECT_ID, makePageRow } from "../testing.js";
import {
  CREATE_PAGE_COMMAND_ID,
  CREATE_PAGE_COMMAND_SEQUENCE,
  CREATE_PAGE_COMMAND_TITLE,
  PageCreateDialogEntry,
  PagesQuickCreate,
} from "./PagesCreateEntry.js";

function withClient(element: React.ReactElement): React.ReactElement {
  return (
    <QueryClientProvider client={new QueryClient({ defaultOptions: { mutations: { retry: false } } })}>
      {element}
    </QueryClientProvider>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  contracts.createPage.mockResolvedValue({ ...makePageRow({ id: PAGES_PAGE_ID }), description_html: "" });
  clearCommands();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => {
  clearCommands();
  resetAppClient();
});

describe("PagesQuickCreate", () => {
  it("is one labelled press with no form", () => {
    const onPress = vi.fn();
    render(<PagesQuickCreate pending={false} onPress={onPress} />);
    const button = screen.getByTestId("pages-quick-create");
    expect(button).toHaveTextContent("Add page");
    expect(button).toBeEnabled();
    fireEvent.click(button);
    expect(onPress).toHaveBeenCalledTimes(1);
  });

  it("shows its progress and refuses a second press while creating", () => {
    const onPress = vi.fn();
    render(<PagesQuickCreate pending onPress={onPress} />);
    expect(screen.getByTestId("pages-quick-create")).toBeDisabled();
    fireEvent.click(screen.getByTestId("pages-quick-create"));
    expect(onPress).not.toHaveBeenCalled();
  });
});

describe("PageCreateDialogEntry", () => {
  const props = {
    workspaceSlug: "acme",
    projectId: PAGES_PROJECT_ID,
    tab: "private" as const,
    onOpenPage: vi.fn(),
    allowed: true,
  };

  it("registers the palette command and opens the dialog on its own visibility", () => {
    render(withClient(<PageCreateDialogEntry {...props} />));
    const command = findCommand(CREATE_PAGE_COMMAND_ID);
    expect(command?.title).toBe(CREATE_PAGE_COMMAND_TITLE);
    expect(command?.title).toBe("New page");
    expect(command?.sequence).toBe(CREATE_PAGE_COMMAND_SEQUENCE);
    expect(command?.sequence).toBe("nd");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    act(() => command?.run());
    expect(screen.getByRole("dialog", { name: "Create page" })).toBeInTheDocument();
    expect(screen.getByTestId("page-create-access-label")).toHaveTextContent("Private");
  });

  it("hands the new page id back and closes after a create", async () => {
    const onOpenPage = vi.fn();
    render(withClient(<PageCreateDialogEntry {...props} onOpenPage={onOpenPage} />));
    act(() => findCommand(CREATE_PAGE_COMMAND_ID)?.run());
    fireEvent.change(screen.getByPlaceholderText("Title"), { target: { value: "Made" } });
    fireEvent.click(screen.getByRole("button", { name: "Create Page" }));

    await waitFor(() => expect(onOpenPage).toHaveBeenCalledWith(PAGES_PAGE_ID));
    expect(contracts.createPage).toHaveBeenCalledWith(expect.anything(), "acme", PAGES_PROJECT_ID, {
      name: "Made",
      access: 1,
    });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("unregisters the command when it unmounts", () => {
    const view = render(withClient(<PageCreateDialogEntry {...props} />));
    expect(findCommand(CREATE_PAGE_COMMAND_ID)).toBeDefined();
    view.unmount();
    expect(findCommand(CREATE_PAGE_COMMAND_ID)).toBeUndefined();
  });

  it("registers nothing and mounts no dialog for a guest", () => {
    render(withClient(<PageCreateDialogEntry {...props} allowed={false} />));
    expect(findCommand(CREATE_PAGE_COMMAND_ID)).toBeUndefined();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
