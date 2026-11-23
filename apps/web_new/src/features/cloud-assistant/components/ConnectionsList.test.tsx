// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Connection cards (CLOUD-032–034): per-kind controls, the permanent
// account row's sign-in prompt, the stale notice, inline removal
// confirmation, and the per-key model-ID editor (NEWPIDASH-394): per-ID
// status, test, failure detail, "Save anyway", and the last-ID removal
// confirmation that deletes the key.
import { ApiError } from "@pidash/api-client";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { lastModelRemoveBody } from "../../assistant/index.js";
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
    models: ["meta-llama/llama-3.3-70b-instruct"],
    modelVerifiedAt: {},
    disconnected: false,
    removable: true,
    disconnectable: false,
    quotaPlan: null,
    modelCredit: null,
    ...overrides,
  };
}

function setup(rows: ConnectionRow[], overrides: Partial<React.ComponentProps<typeof ConnectionsList>> = {}) {
  const props: React.ComponentProps<typeof ConnectionsList> = {
    rows,
    signInAvailable: true,
    signInPending: false,
    testing: false,
    busyId: null,
    topUpHref: null,
    onCommitModel: vi.fn(),
    onSaveModels: vi.fn().mockResolvedValue(undefined),
    onTestModel: vi.fn().mockResolvedValue({ model: "m", status: "verified" as const }),
    onTest: vi.fn(),
    onRemove: vi.fn(),
    onDisconnect: vi.fn(),
    onSignIn: vi.fn(),
    ...overrides,
  };
  render(<ConnectionsList {...props} />);
  return props;
}

/** The editor row naming one model ID. */
function editorRow(model: string): HTMLElement {
  const rows = screen.getAllByTestId("byok-model-row");
  const found = rows.find((row) => within(row).queryByText(model) !== null);
  if (found === undefined) throw new Error(`no editor row for ${model}`);
  return found;
}

function refusedWrite(
  models: { model: string; status: "verified" | "unverified" | "failed"; code?: string; detail?: string }[]
) {
  return new ApiError({
    message: "Model test failed",
    status: 400,
    code: "model_test_failed",
    body: { error: "model_test_failed", models },
  });
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("ConnectionsList", () => {
  it("renders a selected self-supplied row with its model-ID editor and actions", () => {
    setup([row()]);
    expect(screen.getByText("BYOK:openrouter.ai")).toBeInTheDocument();
    expect(screen.getByText("In use")).toBeInTheDocument();
    expect(screen.queryByRole("radio")).toBeNull();
    expect(within(editorRow("meta-llama/llama-3.3-70b-instruct")).getByTestId("byok-model-status")).toHaveTextContent(
      "Not verified"
    );
    expect(screen.queryByRole("button", { name: "Test" })).toBeNull();
    expect(screen.getByRole("button", { name: "Remove" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Disconnect" })).toBeNull();
  });

  it("offers the provider picker on a managed row and tests it", async () => {
    const user = userEvent.setup();
    const props = setup([
      row({
        id: "row-2",
        kind: "codex",
        title: "OpenAI account",
        selected: true,
        removable: false,
        disconnectable: true,
        modelControl: "select",
        modelName: "gpt-5.6-sol",
        modelChoices: [
          { value: "gpt-5.6-sol", label: "gpt-5.6-sol", credit: null, badge: null, selectable: true },
          { value: "gpt-5.5", label: "Previous generation", credit: null, badge: null, selectable: true },
        ],
        markers: [],
      }),
    ]);
    const picker = screen.getByRole("combobox", { name: "Model for OpenAI account" });
    expect(picker).toHaveValue("gpt-5.6-sol");
    expect(screen.getByRole("option", { name: "Previous generation" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Remove" })).toBeNull();
    expect(screen.getByRole("button", { name: "Disconnect" })).toBeInTheDocument();
    await user.selectOptions(picker, "gpt-5.5");
    expect(props.onCommitModel).toHaveBeenCalledWith(expect.objectContaining({ id: "row-2" }), "gpt-5.5");
    await user.click(screen.getByRole("button", { name: "Test" }));
    expect(props.onTest).toHaveBeenCalledTimes(1);
  });

  it("persists an added model ID with the longer list", async () => {
    const user = userEvent.setup();
    const props = setup([row()]);
    const onSaveModels = vi.mocked(props.onSaveModels);
    await user.type(screen.getByLabelText("Add a model ID this key serves"), "gpt-6{Enter}");
    expect(onSaveModels).toHaveBeenCalledWith(
      expect.objectContaining({ id: "row-1" }),
      ["meta-llama/llama-3.3-70b-instruct", "gpt-6"],
      false
    );
  });

  it("persists a removal with the shorter list", async () => {
    const user = userEvent.setup();
    const props = setup([row({ models: ["gpt-6", "gpt-6.2"] })]);
    const onSaveModels = vi.mocked(props.onSaveModels);
    await user.click(within(editorRow("gpt-6.2")).getByRole("button", { name: "Remove gpt-6.2" }));
    expect(props.onRemove).not.toHaveBeenCalled();
    expect(onSaveModels).toHaveBeenCalledWith(expect.objectContaining({ id: "row-1" }), ["gpt-6"], false);
  });

  it("tests one ID and shows the provider message on failure", async () => {
    const user = userEvent.setup();
    const props = setup([row()]);
    const onTestModel = vi.mocked(props.onTestModel);
    onTestModel.mockResolvedValueOnce({
      model: "meta-llama/llama-3.3-70b-instruct",
      status: "failed",
      code: "model_invalid",
      detail: "Model not accepted by the provider.",
    });
    await user.click(
      within(editorRow("meta-llama/llama-3.3-70b-instruct")).getByRole("button", {
        name: "Test meta-llama/llama-3.3-70b-instruct",
      })
    );
    expect(onTestModel).toHaveBeenCalledWith(
      expect.objectContaining({ id: "row-1" }),
      "meta-llama/llama-3.3-70b-instruct"
    );
    expect(within(editorRow("meta-llama/llama-3.3-70b-instruct")).getByTestId("byok-model-detail")).toHaveTextContent(
      "Model not accepted by the provider."
    );
    expect(within(editorRow("meta-llama/llama-3.3-70b-instruct")).getByTestId("byok-model-status")).toHaveTextContent(
      "Failed"
    );
  });

  it("marks a tested-but-unverified ID as not verified", async () => {
    const user = userEvent.setup();
    const props = setup([row()]);
    vi.mocked(props.onTestModel).mockResolvedValueOnce({
      model: "meta-llama/llama-3.3-70b-instruct",
      status: "unverified",
      code: "provider_unreachable",
      detail: "Timed out.",
    });
    await user.click(
      within(editorRow("meta-llama/llama-3.3-70b-instruct")).getByRole("button", {
        name: "Test meta-llama/llama-3.3-70b-instruct",
      })
    );
    expect(within(editorRow("meta-llama/llama-3.3-70b-instruct")).getByTestId("byok-model-detail")).toHaveTextContent(
      "Timed out."
    );
    expect(within(editorRow("meta-llama/llama-3.3-70b-instruct")).getByTestId("byok-model-status")).toHaveTextContent(
      "Not verified"
    );
  });

  it("shows refused-write results per ID and retries with Save anyway for unverified-only refusals", async () => {
    const user = userEvent.setup();
    const props = setup([row({ models: ["gpt-6", "flaky"] })]);
    const onSaveModels = vi.mocked(props.onSaveModels);
    onSaveModels.mockRejectedValueOnce(
      refusedWrite([{ model: "flaky", status: "unverified", code: "provider_unreachable", detail: "Timed out." }])
    );
    await user.type(screen.getByLabelText("Add a model ID this key serves"), "fresh{Enter}");
    expect(within(editorRow("flaky")).getByTestId("byok-model-detail")).toHaveTextContent("Timed out.");
    expect(within(editorRow("flaky")).getByTestId("byok-model-status")).toHaveTextContent("Not verified");
    onSaveModels.mockClear();
    await user.click(screen.getByRole("button", { name: "Save anyway" }));
    expect(onSaveModels).toHaveBeenCalledWith(
      expect.objectContaining({ id: "row-1" }),
      ["gpt-6", "flaky", "fresh"],
      true
    );
  });

  it("blocks the Save-anyway retry while any ID failed", async () => {
    const user = userEvent.setup();
    const props = setup([row({ models: ["bad"] })]);
    vi.mocked(props.onSaveModels).mockRejectedValueOnce(
      refusedWrite([{ model: "bad", status: "failed", code: "model_invalid", detail: "No such model." }])
    );
    await user.type(screen.getByLabelText("Add a model ID this key serves"), "fresh{Enter}");
    expect(within(editorRow("bad")).getByTestId("byok-model-detail")).toHaveTextContent("No such model.");
    expect(within(editorRow("bad")).getByTestId("byok-model-status")).toHaveTextContent("Failed");
    expect(screen.queryByRole("button", { name: "Save anyway" })).toBeNull();
  });

  it("asks for confirmation naming the key before removing the last ID, then deletes the connection", async () => {
    const user = userEvent.setup();
    const props = setup([row({ models: ["gpt-6"] })]);
    await user.click(within(editorRow("gpt-6")).getByRole("button", { name: "Remove gpt-6" }));
    expect(props.onRemove).not.toHaveBeenCalled();
    expect(screen.getByText(lastModelRemoveBody("gpt-6", "BYOK:openrouter.ai"))).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Remove both" }));
    expect(props.onSaveModels).not.toHaveBeenCalled();
    expect(props.onRemove).toHaveBeenCalledWith(expect.objectContaining({ id: "row-1" }));
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
    expect(screen.getByText("OpenAI account")).toBeInTheDocument();
    expect(screen.getByText("Not connected")).toBeInTheDocument();
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
    expect(screen.getByText("OpenHub")).toBeInTheDocument();
  });

  it("links plan-backed rows to the usage page and shows the verified stamp", () => {
    setup([
      row({
        kind: "codex",
        title: "OpenAI account",
        usesChatgptPlan: true,
        verifiedAt: "2026-02-03T04:05:06Z",
        models: [],
      }),
    ]);
    const link = screen.getByRole("link", { name: "Manage plan usage" });
    expect(link).toHaveAttribute("href", "https://chatgpt.com/settings/usage");
    expect(link).toHaveAttribute("rel", "noopener noreferrer");
    expect(screen.getByText("Verified", { exact: false }).closest("p")).toHaveTextContent("2026-02-03T04:05:06Z");
  });

  it("offers no per-row selection control; the surface pickers choose the model", () => {
    setup([row({ selected: false, markers: [] })]);
    expect(screen.queryByRole("radio")).toBeNull();
    expect(screen.getByText("BYOK:openrouter.ai")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Test" })).toBeNull();
  });

  it("locks the row's controls while a write is in flight", () => {
    setup([row()], { busyId: "row-1" });
    expect(screen.getByLabelText("Add a model ID this key serves")).toBeDisabled();
    expect(screen.getByRole("button", { name: "Remove meta-llama/llama-3.3-70b-instruct" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Remove" })).toBeDisabled();
    expect(screen.getByText("Saving connection")).toBeInTheDocument();
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

  it("badges free models and disables paid models for a free-plan user", () => {
    setup(
      [
        row({
          id: "row-openhub",
          kind: "openhub",
          title: "OpenHub",
          removable: false,
          disconnectable: false,
          modelControl: "select",
          modelName: "openhub_free_llm",
          quotaPlan: "free",
          modelCredit: "free",
          modelChoices: [
            {
              value: "openhub_free_llm",
              label: "OpenHub free model",
              credit: "free",
              badge: "Free",
              selectable: true,
            },
            {
              value: "moonshotai/kimi-k3",
              label: "Balanced hosted model",
              credit: "blocked",
              badge: "Needs credit",
              selectable: false,
            },
          ],
          markers: ["Included in your plan", "In use"],
        }),
      ],
      { topUpHref: "https://hub.example.com/account/credits" }
    );
    const picker = screen.getByRole("combobox", { name: "Model for OpenHub" });
    expect(picker).toHaveValue("openhub_free_llm");
    const freeOption = screen.getByRole("option", { name: "OpenHub free model (Free)" });
    const paidOption = screen.getByRole("option", { name: "Balanced hosted model (Needs credit)" });
    expect(freeOption).toBeEnabled();
    expect(paidOption).toBeDisabled();
    expect(screen.queryByRole("link", { name: "Top up" })).toBeNull();
  });

  it("warns and links to top up when the stored model needs credit", () => {
    setup(
      [
        row({
          id: "row-openhub",
          kind: "openhub",
          title: "OpenHub",
          removable: false,
          disconnectable: false,
          modelControl: "select",
          modelName: "moonshotai/kimi-k3",
          quotaPlan: "free",
          modelCredit: "blocked",
          modelChoices: [
            {
              value: "moonshotai/kimi-k3",
              label: "Balanced hosted model",
              credit: "blocked",
              badge: "Needs credit",
              selectable: false,
            },
          ],
          markers: ["Included in your plan", "In use"],
        }),
      ],
      { topUpHref: "https://hub.example.com/account/credits" }
    );
    expect(screen.getByRole("status")).toHaveTextContent("This model needs credit on the free plan.");
    const link = screen.getByRole("link", { name: "Top up" });
    expect(link).toHaveAttribute("href", "https://hub.example.com/account/credits");
  });

  it("omits the top-up link without a wallet address", () => {
    setup(
      [
        row({
          id: "row-openhub",
          kind: "openhub",
          title: "OpenHub",
          removable: false,
          disconnectable: false,
          modelControl: "select",
          modelName: "moonshotai/kimi-k3",
          quotaPlan: "free",
          modelCredit: "blocked",
          modelChoices: [],
          markers: ["Included in your plan", "In use"],
        }),
      ],
      { topUpHref: null }
    );
    expect(screen.getByRole("status")).toHaveTextContent("This model needs credit on the free plan.");
    expect(screen.queryByRole("link", { name: "Top up" })).toBeNull();
  });

  it("shows Uses-credit hints without disabling for a paid plan", () => {
    setup([
      row({
        id: "row-openhub",
        kind: "openhub",
        title: "OpenHub",
        removable: false,
        disconnectable: false,
        modelControl: "select",
        modelName: "moonshotai/kimi-k3",
        quotaPlan: "payg",
        modelCredit: "uses-credit",
        modelChoices: [
          {
            value: "openhub_free_llm",
            label: "OpenHub free model",
            credit: "free",
            badge: "Free",
            selectable: true,
          },
          {
            value: "moonshotai/kimi-k3",
            label: "Balanced hosted model",
            credit: "uses-credit",
            badge: "Uses credit",
            selectable: true,
          },
        ],
        markers: ["Included in your plan", "In use"],
      }),
    ]);
    expect(screen.getByRole("option", { name: "OpenHub free model (Free)" })).toBeEnabled();
    expect(screen.getByRole("option", { name: "Balanced hosted model (Uses credit)" })).toBeEnabled();
    expect(screen.queryByRole("status")).toBeNull();
  });
});
