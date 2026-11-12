// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ApiError } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  createModuleLink: vi.fn(),
  updateModuleLink: vi.fn(),
  deleteModuleLink: vi.fn(),
  listWorkspaceMembers: vi.fn(),
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return {
    ...original,
    createModuleLink: mocks.createModuleLink,
    updateModuleLink: mocks.updateModuleLink,
    deleteModuleLink: mocks.deleteModuleLink,
    listWorkspaceMembers: mocks.listWorkspaceMembers,
  };
});

vi.mock("../../../../shared/shell/toaster.js", () => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));

import { notifyError, notifySuccess } from "../../../../shared/shell/toaster.js";
import { makeDirectoryEntry, makeModuleLink } from "../../testing.js";
import type { ModuleLinkSummary } from "./moduleLinksModel.js";
import { ModuleLinksSection } from "./ModuleLinksSection.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const MODULE = "323e4567-e89b-12d3-a456-426614174002";
const SECOND_LINK = "623e4567-e89b-12d3-a456-426614174005";

function stubClipboard(writeText: ReturnType<typeof vi.fn> = vi.fn().mockResolvedValue(undefined)) {
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
  return writeText;
}

function renderSection(links: readonly ModuleLinkSummary[] = [], canEdit = true) {
  const client = {} as never;
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ModuleLinksSection
        workspaceSlug={SLUG}
        projectId={PROJECT}
        moduleId={MODULE}
        links={links}
        canEdit={canEdit}
        client={client}
      />
    </QueryClientProvider>
  );
  return { section: screen.getByTestId("modules-links-section"), client };
}

beforeEach(() => {
  mocks.listWorkspaceMembers.mockResolvedValue([makeDirectoryEntry()]);
  stubClipboard();
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("ModuleLinksSection", () => {
  it("stays collapsed while empty and expands to the notice with an add action", async () => {
    const user = userEvent.setup();
    const { section } = renderSection();
    const toggle = within(section).getByRole("button", { name: "Links" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(within(section).queryByTestId("modules-links-empty")).not.toBeInTheDocument();
    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(within(section).getByTestId("modules-links-empty")).toHaveTextContent("No links added yet");
    expect(within(section).getByRole("button", { name: "Add link" })).toBeInTheDocument();
    expect(mocks.listWorkspaceMembers).not.toHaveBeenCalled();
    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(within(section).queryByTestId("modules-links-empty")).not.toBeInTheDocument();
  });

  it("hides the add action from guests while keeping the notice", async () => {
    const user = userEvent.setup();
    const { section } = renderSection([], false);
    await user.click(within(section).getByRole("button", { name: "Links" }));
    expect(within(section).getByTestId("modules-links-empty")).toBeInTheDocument();
    expect(within(section).queryByRole("button", { name: "Add link" })).not.toBeInTheDocument();
  });

  it("opens by default with rows once links exist", async () => {
    renderSection([
      makeModuleLink(),
      makeModuleLink({ id: SECOND_LINK, title: null, url: "https://plain.example.com" }),
    ]);
    const section = screen.getByTestId("modules-links-section");
    expect(within(section).getByRole("button", { name: "Links" })).toHaveAttribute("aria-expanded", "true");
    expect(within(section).queryByTestId("modules-links-empty")).not.toBeInTheDocument();
    const rows = within(section).getAllByTestId("modules-link-row");
    expect(rows).toHaveLength(2);
    expect((rows[0] as HTMLElement).querySelector('img[src$="/favicon.ico"]')).not.toBeNull();
    const first = within(rows[0] as HTMLElement).getByRole("link", { name: "Launch plan" });
    expect(first).toHaveAttribute("href", "https://docs.example.com/launch");
    expect(first).toHaveAttribute("target", "_blank");
    expect(first).toHaveAttribute("rel", "noopener noreferrer");
    expect(within(rows[1] as HTMLElement).getByRole("link", { name: "https://plain.example.com" })).toBeInTheDocument();
    expect(await within(section).findAllByText(/Added .* by Ada Lovelace/)).toHaveLength(2);
    expect(mocks.listWorkspaceMembers).toHaveBeenCalled();
  });

  it("lets the user collapse a section that opened by default", async () => {
    const user = userEvent.setup();
    const { section } = renderSection([makeModuleLink()]);
    const toggle = within(section).getByRole("button", { name: "Links" });
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(within(section).queryByTestId("modules-link-row")).not.toBeInTheDocument();
  });

  it("opens when links arrive after mount (NEWFRONT-342)", () => {
    const client = {} as never;
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const tree = (links: readonly ModuleLinkSummary[]) => (
      <QueryClientProvider client={queryClient}>
        <ModuleLinksSection
          workspaceSlug={SLUG}
          projectId={PROJECT}
          moduleId={MODULE}
          links={links}
          canEdit
          client={client}
        />
      </QueryClientProvider>
    );
    const { rerender } = render(tree([]));
    const section = screen.getByTestId("modules-links-section");
    const toggle = within(section).getByRole("button", { name: "Links" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    rerender(tree([makeModuleLink()]));
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(within(section).getByTestId("modules-link-row")).toBeInTheDocument();
  });

  it("copies the URL with the pinned notice and reports a denial", async () => {
    const user = userEvent.setup();
    const writeText = stubClipboard();
    const { section } = renderSection([makeModuleLink()], false);
    const row = within(section).getByTestId("modules-link-row");
    expect(within(row).queryByRole("button", { name: "Edit link" })).not.toBeInTheDocument();
    expect(within(row).queryByRole("button", { name: "Delete link" })).not.toBeInTheDocument();
    await user.click(within(row).getByRole("button", { name: "Copy link" }));
    expect(writeText).toHaveBeenCalledWith("https://docs.example.com/launch");
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Copied to clipboard"));

    stubClipboard(vi.fn().mockRejectedValue(new Error("denied")));
    await user.click(within(row).getByRole("button", { name: "Copy link" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't copy the link", "denied"));
  });

  it("creates a link from the dialog and closes on success", async () => {
    const user = userEvent.setup();
    const created = makeModuleLink({ id: SECOND_LINK, title: "Docs", url: "http://example.com" });
    mocks.createModuleLink.mockResolvedValue(created);
    const { section, client } = renderSection();
    await user.click(within(section).getByRole("button", { name: "Links" }));
    await user.click(within(section).getByRole("button", { name: "Add link" }));
    const dialog = screen.getByRole("dialog", { name: "Add link" });
    await user.type(within(dialog).getByLabelText("URL"), "example.com");
    await user.type(within(dialog).getByLabelText("Display title"), "Docs");
    await user.click(within(dialog).getByTestId("modules-link-submit"));
    await waitFor(() =>
      expect(mocks.createModuleLink).toHaveBeenCalledWith(client, SLUG, PROJECT, MODULE, {
        url: "http://example.com",
        title: "Docs",
      })
    );
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Module link created successfully."));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("keeps the dialog open with its input when the create is refused", async () => {
    const user = userEvent.setup();
    mocks.createModuleLink.mockRejectedValue(
      new ApiError({ code: "http_error", message: "Enter a valid URL.", status: 400 })
    );
    const { section } = renderSection();
    await user.click(within(section).getByRole("button", { name: "Links" }));
    await user.click(within(section).getByRole("button", { name: "Add link" }));
    const dialog = screen.getByRole("dialog", { name: "Add link" });
    await user.type(within(dialog).getByLabelText("URL"), "https://refused.example");
    await user.click(within(dialog).getByTestId("modules-link-submit"));
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith("Couldn't create the module link", "Enter a valid URL.")
    );
    expect(screen.getByRole("dialog", { name: "Add link" })).toBeInTheDocument();
    expect(screen.getByLabelText("URL")).toHaveValue("https://refused.example");
  });

  it("edits a row through the Update dialog and reports a refusal", async () => {
    const user = userEvent.setup();
    const link = makeModuleLink({ title: "Docs" });
    mocks.updateModuleLink.mockResolvedValueOnce({ ...link, title: "Docs v2" });
    mocks.updateModuleLink.mockRejectedValueOnce(new ApiError({ code: "http_error", message: "", status: 500 }));
    const { section, client } = renderSection([link]);
    const row = within(section).getByTestId("modules-link-row");
    await user.click(within(row).getByRole("button", { name: "Edit link" }));
    const dialog = screen.getByRole("dialog", { name: "Update link" });
    await user.clear(within(dialog).getByLabelText("Display title"));
    await user.type(within(dialog).getByLabelText("Display title"), "Docs v2");
    await user.click(within(dialog).getByTestId("modules-link-submit"));
    await waitFor(() =>
      expect(mocks.updateModuleLink).toHaveBeenCalledWith(client, SLUG, PROJECT, MODULE, link.id, {
        url: link.url,
        title: "Docs v2",
      })
    );
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Module link updated successfully."));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());

    await user.click(within(row).getByRole("button", { name: "Edit link" }));
    const retry = screen.getByRole("dialog", { name: "Update link" });
    await user.click(within(retry).getByTestId("modules-link-submit"));
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith(
        "Couldn't update the module link",
        "Check your connection and try again."
      )
    );
    expect(screen.getByRole("dialog", { name: "Update link" })).toBeInTheDocument();
  });

  it("returns to a blank Add dialog after a cancelled edit", async () => {
    const user = userEvent.setup();
    const { section } = renderSection([makeModuleLink()]);
    const row = within(section).getByTestId("modules-link-row");
    await user.click(within(row).getByRole("button", { name: "Edit link" }));
    expect(screen.getByRole("dialog", { name: "Update link" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    await user.click(within(section).getByRole("button", { name: "Add link" }));
    const dialog = screen.getByRole("dialog", { name: "Add link" });
    expect(within(dialog).getByLabelText("URL")).toHaveValue("");
  });

  it("deletes a row without asking and reports a refusal", async () => {
    const user = userEvent.setup();
    const link = makeModuleLink();
    mocks.deleteModuleLink.mockResolvedValueOnce(undefined);
    mocks.deleteModuleLink.mockRejectedValueOnce(new ApiError({ code: "http_error", message: "Gone.", status: 404 }));
    const { section, client } = renderSection([link]);
    const row = within(section).getByTestId("modules-link-row");
    await user.click(within(row).getByRole("button", { name: "Delete link" }));
    await waitFor(() => expect(mocks.deleteModuleLink).toHaveBeenCalledWith(client, SLUG, PROJECT, MODULE, link.id));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Module link deleted successfully."));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    await user.click(within(row).getByRole("button", { name: "Delete link" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't delete the module link", "Gone."));
  });

  it("renders archived rows without meta and falls back for unparseable URLs", () => {
    const archived = { id: "723e4567-e89b-12d3-a456-426614174006", title: "Old", url: "not a url" };
    const { section } = renderSection([archived]);
    const row = within(section).getByTestId("modules-link-row");
    expect(within(row).getByRole("link", { name: "Old" })).toHaveAttribute("href", "not a url");
    expect(within(row).queryByText(/Added /)).not.toBeInTheDocument();
    expect(row.querySelector("img")).toBeNull();
    expect(mocks.listWorkspaceMembers).not.toHaveBeenCalled();
  });

  it("shows the age alone when the author left the directory", async () => {
    mocks.listWorkspaceMembers.mockResolvedValue([]);
    renderSection([makeModuleLink()]);
    const section = screen.getByTestId("modules-links-section");
    expect(await within(section).findByText(/^Added /)).toBeInTheDocument();
    expect(within(section).queryByText(/by Ada Lovelace/)).not.toBeInTheDocument();
  });
});
