// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { OpenhubApp, OpenhubAuthStatus } from "@pidash/api-client";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import {
  CONNECT_DIALOG_CANCEL,
  CONNECT_DIALOG_SETUP_LINK,
  CONNECT_DIALOG_SUBMIT,
  ConnectDialog,
  connectDialogTitle,
} from "./ConnectDialog.js";

const APP: OpenhubApp = { appId: "app-manual", slug: "manu", name: "Manual CRM", authType: "api_key" };

const MANIFEST_AUTH: OpenhubAuthStatus = {
  type: "api_key",
  status: "needs_auth",
  manualSetup: {
    setupUrl: "https://crm.example.com/keys",
    instructions_md: "Paste your **API key** below.\n<script>alert(1)</script>",
    fields: [
      { key: "api_key", label: "API key", type: "secret", placeholder: "sk-...", validation: "^sk-[a-z0-9]{4,}$" },
      { key: "region", label: "Region", type: "text", placeholder: "us" },
    ],
  },
};

function renderDialog(auth: OpenhubAuthStatus = MANIFEST_AUTH) {
  const onSubmit = vi.fn(async (_fields: Record<string, string>) => true);
  const onClose = vi.fn(() => {});
  render(<ConnectDialog app={APP} auth={auth} onSubmit={onSubmit} onClose={onClose} />);
  return { onSubmit, onClose };
}

function dialog(): HTMLElement {
  return screen.getByRole("dialog", { name: connectDialogTitle(APP.name) });
}

describe("ConnectDialog", () => {
  it("titles the dialog with the app name (CLOUD-027)", () => {
    renderDialog();
    expect(connectDialogTitle(APP.name)).toContain("Manual CRM");
    expect(dialog()).toBeInTheDocument();
  });

  it("builds fields from the manifest with masking and placeholders (CLOUD-027)", () => {
    renderDialog();
    const scope = within(dialog());
    const key = scope.getByLabelText("API key");
    expect(key).toHaveAttribute("type", "password");
    expect(key).toHaveAttribute("placeholder", "sk-...");
    const region = scope.getByLabelText("Region");
    expect(region).toHaveAttribute("type", "text");
    expect(region).toHaveAttribute("placeholder", "us");
  });

  it("renders instructions as plain text, never markup (CLOUD-027)", () => {
    renderDialog();
    const scope = within(dialog());
    expect(scope.getByText(/Paste your API key below\./)).toBeInTheDocument();
    expect(scope.queryByText(/\*\*/)).not.toBeInTheDocument();
    expect(scope.getByText(/<script>alert\(1\)<\/script>/)).toBeInTheDocument();
    expect(document.querySelector("script")).toBeNull();
  });

  it("omits instructions the manifest does not carry", () => {
    renderDialog({ type: "api_key", status: "needs_auth", manualSetup: { fields: [] } });
    expect(within(dialog()).queryByText(/Paste your/)).not.toBeInTheDocument();
  });

  it("links the issuance address only for safely absolute HTTPS (CLOUD-027)", () => {
    const { unmount } = render(
      <ConnectDialog app={APP} auth={MANIFEST_AUTH} onSubmit={async () => true} onClose={() => {}} />
    );
    expect(screen.getByRole("link", { name: CONNECT_DIALOG_SETUP_LINK })).toHaveAttribute(
      "href",
      "https://crm.example.com/keys"
    );
    unmount();

    for (const setupUrl of [
      "http://crm.example.com/keys",
      "https://user:pass@crm.example.com/keys",
      "/relative/keys",
    ]) {
      const view = render(
        <ConnectDialog
          app={APP}
          auth={{ type: "api_key", status: "needs_auth", manualSetup: { setupUrl, fields: [] } }}
          onSubmit={async () => true}
          onClose={() => {}}
        />
      );
      expect(screen.queryByRole("link", { name: CONNECT_DIALOG_SETUP_LINK })).not.toBeInTheDocument();
      view.unmount();
    }

    render(
      <ConnectDialog
        app={APP}
        auth={{ type: "api_key", status: "needs_auth" }}
        onSubmit={async () => true}
        onClose={() => {}}
      />
    );
    expect(screen.queryByRole("link", { name: CONNECT_DIALOG_SETUP_LINK })).not.toBeInTheDocument();
  });

  it("falls back to one secret field for a bare key type (CLOUD-027)", () => {
    renderDialog({ type: "api_key", status: "needs_auth" });
    const scope = within(dialog());
    expect(scope.getByLabelText("API key")).toHaveAttribute("type", "password");
    expect((scope.getByLabelText("API key") as HTMLInputElement).placeholder).toBe("");
    expect(scope.queryByLabelText("Region")).not.toBeInTheDocument();
  });

  it("blocks empty submits inline with every problem at once (CLOUD-027)", () => {
    const { onSubmit } = renderDialog();
    fireEvent.click(screen.getByRole("button", { name: CONNECT_DIALOG_SUBMIT }));
    expect(within(dialog()).getAllByRole("alert")).toHaveLength(2);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("blocks malformed values inline and keeps the dialog open (CLOUD-027)", () => {
    const { onSubmit, onClose } = renderDialog();
    fireEvent.change(within(dialog()).getByLabelText("API key"), { target: { value: "nope" } });
    fireEvent.change(within(dialog()).getByLabelText("Region"), { target: { value: "us" } });
    fireEvent.click(screen.getByRole("button", { name: CONNECT_DIALOG_SUBMIT }));
    expect(within(dialog()).getAllByRole("alert")).toHaveLength(1);
    expect(onSubmit).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
    expect(dialog()).toBeInTheDocument();
  });

  it("posts trimmed values, clears secrets, and closes on success (CLOUD-027)", async () => {
    const { onSubmit, onClose } = renderDialog();
    fireEvent.change(within(dialog()).getByLabelText("API key"), { target: { value: "  sk-abcd99  " } });
    fireEvent.change(within(dialog()).getByLabelText("Region"), { target: { value: "us" } });
    fireEvent.click(screen.getByRole("button", { name: CONNECT_DIALOG_SUBMIT }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledWith({ api_key: "sk-abcd99", region: "us" }));
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    expect((within(dialog()).getByLabelText("API key") as HTMLInputElement).value).toBe("");
  });

  it("clears secrets but keeps other values when the submit fails (CLOUD-027)", async () => {
    const onSubmit = vi.fn(async (_fields: Record<string, string>) => false);
    const onClose = vi.fn(() => {});
    render(<ConnectDialog app={APP} auth={MANIFEST_AUTH} onSubmit={onSubmit} onClose={onClose} />);
    fireEvent.change(within(dialog()).getByLabelText("API key"), { target: { value: "sk-wrong1" } });
    fireEvent.change(within(dialog()).getByLabelText("Region"), { target: { value: "us" } });
    fireEvent.click(screen.getByRole("button", { name: CONNECT_DIALOG_SUBMIT }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledWith({ api_key: "sk-wrong1", region: "us" }));
    await waitFor(() => expect((within(dialog()).getByLabelText("API key") as HTMLInputElement).value).toBe(""));
    expect((within(dialog()).getByLabelText("Region") as HTMLInputElement).value).toBe("us");
    expect(onClose).not.toHaveBeenCalled();
    expect(dialog()).toBeInTheDocument();
  });

  it("submits once while the first submit is in flight", async () => {
    let release!: (ok: boolean) => void;
    const gate = new Promise<boolean>((resolve) => {
      release = resolve;
    });
    const onSubmit = vi.fn(() => gate);
    render(<ConnectDialog app={APP} auth={MANIFEST_AUTH} onSubmit={onSubmit} onClose={() => {}} />);
    fireEvent.change(within(dialog()).getByLabelText("API key"), { target: { value: "sk-abcd99" } });
    fireEvent.change(within(dialog()).getByLabelText("Region"), { target: { value: "us" } });

    const submit = screen.getByRole("button", { name: CONNECT_DIALOG_SUBMIT });
    fireEvent.click(submit);
    await waitFor(() => expect(submit).toBeDisabled());
    fireEvent.click(submit);
    expect(onSubmit).toHaveBeenCalledTimes(1);

    release(true);
    await waitFor(() => expect(submit).not.toBeDisabled());
  });

  it("hides inline problems while a submit is in flight (CLOUD-027)", async () => {
    let release!: (ok: boolean) => void;
    const gate = new Promise<boolean>((resolve) => {
      release = resolve;
    });
    const onSubmit = vi.fn(() => gate);
    render(<ConnectDialog app={APP} auth={MANIFEST_AUTH} onSubmit={onSubmit} onClose={() => {}} />);
    fireEvent.change(within(dialog()).getByLabelText("API key"), { target: { value: "sk-abcd99" } });
    fireEvent.change(within(dialog()).getByLabelText("Region"), { target: { value: "us" } });
    fireEvent.click(screen.getByRole("button", { name: CONNECT_DIALOG_SUBMIT }));
    // Secrets clear at submit start; the cleared secret must not surface
    // its required-error while the request flies — the rejection toast
    // owns that moment instead.
    await waitFor(() => expect((within(dialog()).getByLabelText("API key") as HTMLInputElement).value).toBe(""));
    expect(within(dialog()).queryAllByRole("alert")).toHaveLength(0);
    release(false);
    await waitFor(() => expect(within(dialog()).getAllByRole("alert")).toHaveLength(1));
  });

  it("cancels and dismisses through onClose (CLOUD-027)", async () => {
    const { onClose } = renderDialog();
    fireEvent.click(screen.getByRole("button", { name: CONNECT_DIALOG_CANCEL }));
    expect(onClose).toHaveBeenCalledTimes(1);

    const user = userEvent.setup();
    await user.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledTimes(2);
  });
});
