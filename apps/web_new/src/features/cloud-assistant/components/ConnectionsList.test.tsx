// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Connection cards (CLOUD-032–034): per-kind controls, the permanent
// account row's sign-in prompt, the stale notice, inline removal
// confirmation, and the free-text model commit on blur and Enter.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { ConnectionRow } from "../lib/rows.js";
import { ConnectionsList } from "./ConnectionsList.js";

function row(overrides: Partial<ConnectionRow> = {}): ConnectionRow {
  return {
    id: "row-1",
    kind: "byok",
    title: "BYOK:openrouter.ai",
    selected: true,
    selectable: true,
    markers: ["In use"],
    stale: false,
    usesChatgptPlan: false,
    modelControl: "input",
    modelName: "meta-llama/llama-3.3-70b-instruct",
    modelChoices: [],
    modelSuggestions: ["anthropic/claude-sonnet-4-6"],
    verifiedAt: null,
    disconnected: false,
    removable: true,
    disconnectable: false,
    ...overrides,
  };
}

function setup(rows: ConnectionRow[], overrides: Partial<React.ComponentProps<typeof ConnectionsList>> = {}) {
  const props = {
    rows,
    signInAvailable: true,
    signInPending: false,
    testing: false,
    busyId: null,
    onSelect: vi.fn(),
    onCommitModel: vi.fn(),
    onTest: vi.fn(),
    onRemove: vi.fn(),
    onDisconnect: vi.fn(),
    onSignIn: vi.fn(),
    ...overrides,
  };
  render(<ConnectionsList {...props} />);
  return props;
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("ConnectionsList", () => {
  it("renders a selected self-supplied row with its markers, suggestion list and actions", () => {
    setup([row()]);
    expect(screen.getByText("BYOK:openrouter.ai")).toBeInTheDocument();
    expect(screen.getByText("In use")).toBeInTheDocument();
    const radio = screen.getByRole("radio", { name: "BYOK:openrouter.ai" });
    expect(radio).toBeChecked();
    expect(radio).toBeEnabled();
    expect(screen.getByRole("combobox", { name: "Model for BYOK:openrouter.ai" })).toHaveValue(
      "meta-llama/llama-3.3-70b-instruct"
    );
    expect(screen.getByRole("button", { name: "Test" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Remove" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Disconnect" })).toBeNull();
  });

  it("offers the provider picker on a managed row and hides Test on an inactive one", () => {
    setup([
      row({
        id: "row-2",
        kind: "codex",
        title: "OpenAI account",
        selected: false,
        removable: false,
        disconnectable: true,
        modelControl: "select",
        modelName: "gpt-5.6-sol",
        modelChoices: [
          { value: "gpt-5.6-sol", label: "gpt-5.6-sol" },
          { value: "gpt-5.5", label: "Previous generation" },
        ],
        markers: [],
      }),
    ]);
    const picker = screen.getByRole("combobox", { name: "Model for OpenAI account" });
    expect(picker).toHaveValue("gpt-5.6-sol");
    expect(screen.getByRole("option", { name: "Previous generation" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Test" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Remove" })).toBeNull();
    expect(screen.getByRole("button", { name: "Disconnect" })).toBeInTheDocument();
  });

  it("commits a managed model pick", async () => {
    const user = userEvent.setup();
    const props = setup([
      row({
        id: "row-2",
        kind: "codex",
        title: "OpenAI account",
        modelControl: "select",
        modelName: "gpt-5.6-sol",
        modelChoices: [
          { value: "gpt-5.6-sol", label: "gpt-5.6-sol" },
          { value: "gpt-5.5", label: "Previous generation" },
        ],
      }),
    ]);
    await user.selectOptions(screen.getByRole("combobox", { name: "Model for OpenAI account" }), "gpt-5.5");
    expect(props.onCommitModel).toHaveBeenCalledWith(expect.objectContaining({ id: "row-2" }), "gpt-5.5");
  });

  it("commits a free-text model on Enter and on blur, but not for an unchanged value", async () => {
    const user = userEvent.setup();
    const props = setup([row()]);
    const onCommitModel = vi.mocked(props.onCommitModel);
    const input = screen.getByRole("combobox", { name: "Model for BYOK:openrouter.ai" });
    await user.clear(input);
    await user.type(input, "  llama-edited  {Enter}");
    expect(onCommitModel).toHaveBeenCalledWith(expect.objectContaining({ id: "row-1" }), "llama-edited");
    onCommitModel.mockClear();
    await user.clear(input);
    await user.type(input, "other");
    await user.tab();
    expect(onCommitModel).toHaveBeenCalledWith(expect.objectContaining({ id: "row-1" }), "other");
    onCommitModel.mockClear();
    await user.tab();
    expect(props.onCommitModel).not.toHaveBeenCalled();
  });

  it("does not commit an emptied free-text model", async () => {
    const user = userEvent.setup();
    const props = setup([row()]);
    const input = screen.getByRole("combobox", { name: "Model for BYOK:openrouter.ai" });
    await user.clear(input);
    await user.keyboard("{Enter}");
    await user.tab();
    expect(props.onCommitModel).not.toHaveBeenCalled();
  });

  it("asks inline before removing a self-supplied row", async () => {
    const user = userEvent.setup();
    const props = setup([row()]);
    await user.click(screen.getByRole("button", { name: "Remove" }));
    expect(props.onRemove).not.toHaveBeenCalled();
    const confirm = screen.getByRole("group", { name: "Remove this connection?" });
    expect(confirm).toHaveTextContent("Its key is deleted right away.");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("group", { name: "Remove this connection?" })).toBeNull();
    expect(props.onRemove).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Remove" }));
    await user.click(screen.getByRole("button", { name: "Confirm remove" }));
    expect(props.onRemove).toHaveBeenCalledWith(expect.objectContaining({ id: "row-1" }));
    expect(screen.queryByRole("group", { name: "Remove this connection?" })).toBeNull();
  });

  it("blocks a disconnected account row and prompts for sign-in", () => {
    setup([
      row({
        id: "row-3",
        kind: "codex",
        title: "OpenAI account",
        selected: false,
        selectable: false,
        disconnected: true,
        removable: false,
        disconnectable: false,
        modelControl: "hidden",
        markers: ["Not connected"],
      }),
    ]);
    expect(screen.getByRole("radio", { name: "OpenAI account" })).toBeDisabled();
    expect(screen.getByText(/Connecting an OpenAI account/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Continue with ChatGPT" })).toBeInTheDocument();
    expect(screen.queryByRole("combobox")).toBeNull();
  });

  it("names the deployment limit when sign-in is unavailable", () => {
    setup(
      [
        row({
          id: "row-3",
          kind: "codex",
          title: "OpenAI account",
          selected: false,
          selectable: false,
          disconnected: true,
          removable: false,
          disconnectable: false,
          modelControl: "hidden",
        }),
      ],
      { signInAvailable: false }
    );
    expect(screen.getByText("OpenAI sign-in is switched off for this deployment.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Continue with ChatGPT" })).toBeNull();
  });

  it("shows the pending sign-in state on the account row's action", () => {
    setup(
      [
        row({
          kind: "codex",
          title: "OpenAI account",
          selectable: false,
          disconnected: true,
          removable: false,
          disconnectable: false,
          modelControl: "hidden",
        }),
      ],
      { signInPending: true }
    );
    expect(screen.getByRole("button", { name: "Sign-in in progress…" })).toBeDisabled();
  });

  it("carries the stale notice on a hosted row the session cannot reach", () => {
    setup([
      row({
        id: "row-4",
        kind: "openhub",
        title: "OpenHub",
        selected: false,
        selectable: false,
        stale: true,
        modelControl: "hidden",
        markers: ["Included in your plan"],
        removable: false,
        disconnectable: false,
      }),
    ]);
    expect(screen.getByRole("status")).toHaveTextContent("Sign out and back in");
    expect(screen.getByRole("radio", { name: "OpenHub" })).toBeDisabled();
  });

  it("links plan-backed rows to the usage page and shows the verified stamp", () => {
    setup([row({ kind: "codex", title: "OpenAI account", usesChatgptPlan: true, verifiedAt: "2026-02-03T04:05:06Z" })]);
    const link = screen.getByRole("link", { name: "Manage plan usage" });
    expect(link).toHaveAttribute("href", "https://chatgpt.com/settings/usage");
    expect(link).toHaveAttribute("rel", "noopener noreferrer");
    expect(screen.getByText("Verified", { exact: false }).closest("p")).toHaveTextContent("2026-02-03T04:05:06Z");
  });

  it("reports a selection and disables the row while it is busy", async () => {
    const user = userEvent.setup();
    const props = setup([row({ selected: false, markers: [] })]);
    await user.click(screen.getByRole("radio", { name: "BYOK:openrouter.ai" }));
    expect(props.onSelect).toHaveBeenCalledWith(expect.objectContaining({ id: "row-1" }));
    expect(screen.queryByRole("button", { name: "Test" })).toBeNull();
  });

  it("locks the row's controls while a write is in flight", () => {
    setup([row()], { busyId: "row-1" });
    expect(screen.getByRole("radio", { name: "BYOK:openrouter.ai" })).toBeDisabled();
    expect(screen.getByRole("combobox", { name: "Model for BYOK:openrouter.ai" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Remove" })).toBeDisabled();
    expect(screen.getByText("Saving connection")).toBeInTheDocument();
  });

  it("runs the test action", async () => {
    const user = userEvent.setup();
    const props = setup([row()]);
    await user.click(screen.getByRole("button", { name: "Test" }));
    expect(props.onTest).toHaveBeenCalledTimes(1);
  });

  it("starts provider sign-in from the disconnected account row", async () => {
    const user = userEvent.setup();
    const props = setup([
      row({
        kind: "codex",
        title: "OpenAI account",
        selectable: false,
        disconnected: true,
        removable: false,
        disconnectable: false,
        modelControl: "hidden",
      }),
    ]);
    await user.click(screen.getByRole("button", { name: "Continue with ChatGPT" }));
    expect(props.onSignIn).toHaveBeenCalledTimes(1);
  });
});
