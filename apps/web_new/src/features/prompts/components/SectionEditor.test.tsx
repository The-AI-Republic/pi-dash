// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ResolvedPromptSection } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const { contractMocks } = vi.hoisted(() => ({
  contractMocks: { upsertPromptSection: vi.fn(), revertPromptSection: vi.fn(), previewPrompt: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

vi.mock("../../../shared/shell/index.js", () => ({
  notifySuccess: vi.fn(),
}));

import { REVERT_FAILED_MESSAGE } from "../api/mutations.js";
import type { MergedPromptSection } from "../lib/sections.js";
import { SectionEditor } from "./SectionEditor.js";

const SLUG = "acme";

function row(overrides: Partial<ResolvedPromptSection> = {}): ResolvedPromptSection {
  return {
    key: "autonomy",
    title: "Autonomy",
    customizable: "overridable",
    body: "effective body",
    default_body: "pristine default",
    source: "default",
    version: 1,
    needs_attention: false,
    editable_at_workspace: true,
    editable_at_personal: true,
    ...overrides,
  };
}

function section(overrides: Partial<MergedPromptSection> = {}): MergedPromptSection {
  return {
    key: "autonomy",
    title: "Autonomy",
    kinds: ["coding-task"],
    user: row(),
    workspace: row({ source: "default", body: "baseline body" }),
    ...overrides,
  };
}

function renderEditor(props: { scope?: "workspace" | "user"; section?: MergedPromptSection } = {}) {
  const onClose = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <SectionEditor
        workspaceSlug={SLUG}
        section={props.section ?? section()}
        scope={props.scope ?? "user"}
        onClose={onClose}
      />
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

describe("SectionEditor", () => {
  it("edits on a plain labelled text surface without kit field scaffolding", () => {
    renderEditor({ scope: "user" });
    const surface = screen.getByLabelText("Prompt section text");
    expect(surface.tagName).toBe("TEXTAREA");
    expect(surface).toHaveAttribute("rows", "10");
    expect(surface.className).toContain("resize-y");
    expect(surface.className).toContain("disabled:opacity-50");
  });

  it("seeds the workspace draft from the baseline with Save disarmed", () => {
    renderEditor({ scope: "workspace" });
    expect(screen.getByText("Workspace default")).toBeInTheDocument();
    expect(screen.getByLabelText("Prompt section text")).toHaveValue("baseline body");
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  });

  it("seeds the personal draft from the effective body", () => {
    renderEditor({ scope: "user", section: section({ user: row({ body: "my effective" }) }) });
    expect(screen.getByText("Your personal override")).toBeInTheDocument();
    expect(screen.getByLabelText("Prompt section text")).toHaveValue("my effective");
  });

  it("saves a dirty draft and closes", async () => {
    contractMocks.upsertPromptSection.mockResolvedValue({ section_key: "autonomy" });
    const user = userEvent.setup();
    const { onClose } = renderEditor({ scope: "workspace" });
    await user.type(screen.getByLabelText("Prompt section text"), " plus more");
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    expect(contractMocks.upsertPromptSection).toHaveBeenCalledWith(expect.anything(), SLUG, "autonomy", {
      scope: "workspace",
      body: "baseline body plus more",
    });
  });

  it("keeps Save disarmed while the save is in flight", async () => {
    contractMocks.upsertPromptSection.mockReturnValue(new Promise<never>(() => {}));
    const user = userEvent.setup();
    renderEditor({ scope: "user" });
    await user.type(screen.getByLabelText("Prompt section text"), "x");
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Save" })).toBeDisabled());
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
  });

  it("lands save failures inline with the server detail and keeps the draft", async () => {
    contractMocks.upsertPromptSection.mockRejectedValue(new Error("parity upsert failure"));
    const user = userEvent.setup();
    const { onClose } = renderEditor({ scope: "user" });
    await user.clear(screen.getByLabelText("Prompt section text"));
    await user.type(screen.getByLabelText("Prompt section text"), "never persisted");
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent("Could not save the section. parity upsert failure")
    );
    expect(screen.getByLabelText("Prompt section text")).toHaveValue("never persisted");
    expect(onClose).not.toHaveBeenCalled();
  });

  it("compares the draft against the pristine default", async () => {
    const user = userEvent.setup();
    renderEditor({ scope: "user" });
    expect(screen.queryByText("pristine default")).not.toBeInTheDocument();
    await user.type(screen.getByLabelText("Prompt section text"), " draft words");
    await user.click(screen.getByRole("button", { name: "Show default" }));
    expect(screen.getByText("pristine default")).toBeInTheDocument();
    expect(screen.getByLabelText("Prompt section text")).toHaveValue("effective body draft words");
    await user.click(screen.getByRole("button", { name: "Hide default" }));
    expect(screen.queryByText("pristine default")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Prompt section text")).toHaveValue("effective body draft words");
  });

  it("cancels without saving", async () => {
    const user = userEvent.setup();
    const { onClose } = renderEditor({ scope: "user" });
    await user.type(screen.getByLabelText("Prompt section text"), " discarded");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(contractMocks.upsertPromptSection).not.toHaveBeenCalled();
  });

  it("hides the revert affordance without an override at the scope", () => {
    renderEditor({ scope: "user" });
    expect(screen.queryByRole("button", { name: "Revert to default" })).not.toBeInTheDocument();
  });

  it("reverts through the confirmation dialog and closes", async () => {
    contractMocks.revertPromptSection.mockResolvedValue(204);
    const user = userEvent.setup();
    const { onClose } = renderEditor({
      scope: "user",
      section: section({ user: row({ source: "user:abc", body: "mine" }) }),
    });
    await user.click(screen.getByRole("button", { name: "Revert to default" }));
    expect(screen.getByRole("heading", { name: "Revert to default?" })).toBeInTheDocument();
    // Cancelling keeps the editor and the draft.
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Prompt section text")).toHaveValue("mine");
    // Confirming deactivates and closes.
    await user.click(screen.getByRole("button", { name: "Revert to default" }));
    await user.click(screen.getByRole("button", { name: "Revert" }));
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    expect(contractMocks.revertPromptSection).toHaveBeenCalledWith(expect.anything(), SLUG, "autonomy", "user");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("lands revert failures inline with the generic message", async () => {
    contractMocks.revertPromptSection.mockRejectedValue(new Error("parity revert failure"));
    const user = userEvent.setup();
    const { onClose } = renderEditor({
      scope: "workspace",
      section: section({ workspace: row({ source: "workspace", body: "baseline" }) }),
    });
    await user.click(screen.getByRole("button", { name: "Revert to default" }));
    await user.click(screen.getByRole("button", { name: "Revert" }));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent(REVERT_FAILED_MESSAGE));
    expect(screen.getByRole("alert").textContent).not.toContain("parity revert failure");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("falls back to the baseline default when the effective row is missing", async () => {
    const user = userEvent.setup();
    renderEditor({
      scope: "workspace",
      section: section({ user: null, workspace: row({ body: "baseline body", default_body: "baseline default" }) }),
    });
    expect(screen.getByLabelText("Prompt section text")).toHaveValue("baseline body");
    await user.click(screen.getByRole("button", { name: "Show default" }));
    expect(screen.getByText("baseline default")).toBeInTheDocument();
  });

  it("offers no kind switcher for a single-kind section", () => {
    renderEditor({ scope: "user" });
    expect(screen.queryByTestId("draft-preview-kind-coding-task")).not.toBeInTheDocument();
    expect(screen.getByText("Draft preview")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Preview draft" })).toBeDisabled();
  });

  it("switches preview kinds when the section spans several kinds", async () => {
    contractMocks.previewPrompt.mockResolvedValue({ kind: "review", prompt: "review draft render" });
    const user = userEvent.setup();
    renderEditor({
      scope: "user",
      section: section({ kinds: ["coding-task", "review"] }),
    });
    expect(screen.getByTestId("draft-preview-kind-coding-task")).toHaveAttribute("aria-pressed", "true");
    await user.click(screen.getByTestId("draft-preview-kind-review"));
    expect(screen.getByTestId("draft-preview-kind-review")).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByTestId("draft-preview-kind-coding-task")).toHaveAttribute("aria-pressed", "false");
    await user.type(screen.getByPlaceholderText("Issue id (UUID)"), "issue-9");
    await user.click(screen.getByRole("button", { name: "Preview draft" }));
    await waitFor(() => expect(contractMocks.previewPrompt).toHaveBeenCalledTimes(1));
    expect(contractMocks.previewPrompt).toHaveBeenCalledWith(expect.anything(), SLUG, "review", {
      issue_id: "issue-9",
      scope: "user",
      section_key: "autonomy",
      body: "effective body",
    });
    await waitFor(() => expect(screen.getByTestId("draft-preview-result")).toHaveTextContent("review draft render"));
  });

  it("previews the in-progress edit, not the seed, for a scheduler section", async () => {
    contractMocks.previewPrompt.mockResolvedValue({ kind: "scheduler", prompt: "scheduler render" });
    const user = userEvent.setup();
    renderEditor({
      scope: "workspace",
      section: section({ kinds: ["scheduler"], workspace: row({ body: "baseline body" }) }),
    });
    await user.clear(screen.getByLabelText("Prompt section text"));
    await user.type(screen.getByLabelText("Prompt section text"), "unsaved edit");
    await user.type(screen.getByPlaceholderText("Scheduler install id (UUID)"), "binding-7");
    await user.click(screen.getByRole("button", { name: "Preview draft" }));
    await waitFor(() =>
      expect(contractMocks.previewPrompt).toHaveBeenCalledWith(expect.anything(), SLUG, "scheduler", {
        binding_id: "binding-7",
        scope: "workspace",
        section_key: "autonomy",
        body: "unsaved edit",
      })
    );
  });

  it("lands draft-preview failures inline without disturbing the draft", async () => {
    contractMocks.previewPrompt.mockRejectedValue(new Error("parity draft render failure"));
    const user = userEvent.setup();
    renderEditor({ scope: "user" });
    await user.type(screen.getByPlaceholderText("Issue id (UUID)"), "issue-1");
    await user.click(screen.getByRole("button", { name: "Preview draft" }));
    await waitFor(() =>
      expect(screen.getByTestId("draft-preview-error")).toHaveTextContent("parity draft render failure")
    );
    expect(screen.getByLabelText("Prompt section text")).toHaveValue("effective body");
  });

  it("renders an empty default without any row", async () => {
    const user = userEvent.setup();
    renderEditor({ scope: "user", section: section({ user: null, workspace: null }) });
    await user.click(screen.getByRole("button", { name: "Show default" }));
    expect(screen.getByText("Default")).toBeInTheDocument();
  });
});
