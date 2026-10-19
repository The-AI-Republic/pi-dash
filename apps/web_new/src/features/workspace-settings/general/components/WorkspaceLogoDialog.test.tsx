// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Logo dialog (WSET-004): accepted and refused files, the footer states,
// Escape/backdrop closing, the upload flow closing with no notice, the
// failure keeping the dialog, and removal by asset path or legacy URL.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { workspaceDetailsKey } from "../api/details.js";
import { WorkspaceLogoDialog } from "./WorkspaceLogoDialog.js";

vi.mock("../../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { notifyError, notifySuccess } from "../../../../shared/shell/index.js";

const UUID = "123e4567-e89b-12d3-a456-426614174000";
const ASSET_PATH = "/api/assets/v2/workspaces/acme/";
const DETAILS_PATH = "/api/workspaces/acme/";

function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) } as TransportResponse;
}

function stubApi(options: { grantStatus?: number } = {}) {
  const calls: string[] = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string }) => {
    const method = init.method ?? "GET";
    calls.push(`${method} ${url}`);
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.endsWith(ASSET_PATH) && method === "POST") {
      if ((options.grantStatus ?? 200) !== 200) return jsonResponse(url, 500, {});
      return jsonResponse(url, 200, {
        asset_id: "asset-1",
        asset_url: "/api/assets/v2/static/asset-1/",
        upload_data: { url: "https://uploads.test/signed", fields: {} },
      });
    }
    if (url.includes(ASSET_PATH) && (method === "PATCH" || method === "DELETE")) return jsonResponse(url, 204, {});
    if (url.includes("/file-assets/") && method === "DELETE") return jsonResponse(url, 204, {});
    if (url.endsWith(DETAILS_PATH) && method === "PATCH") {
      return jsonResponse(url, 200, {
        id: UUID,
        name: "Acme",
        slug: "acme",
        logo: null,
        logo_asset: null,
        logo_url: null,
        owner: UUID,
        organization_size: null,
        timezone: "UTC",
        background_color: "#fff",
        total_members: 1,
        created_by: null,
        updated_by: null,
        created_at: "2024-01-02T03:04:05Z",
        updated_at: "2024-02-03T04:05:06Z",
      });
    }
    throw new Error(`unexpected fetch ${method} ${url}`);
  }) as never);
  // Browser-side signed upload (defaultByteUpload uses global fetch).
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => ({ ok: true, status: 204 }))
  );
  return { calls };
}

function renderDialog(overrides: Partial<React.ComponentProps<typeof WorkspaceLogoDialog>> = {}) {
  const onOpenChange = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const props: React.ComponentProps<typeof WorkspaceLogoDialog> = {
    workspaceSlug: "acme",
    workspaceId: UUID,
    logoUrl: null,
    open: true,
    onOpenChange,
    ...overrides,
  };
  render(
    <QueryClientProvider client={queryClient}>
      <WorkspaceLogoDialog {...props} />
    </QueryClientProvider>
  );
  return { props, onOpenChange, queryClient };
}

function fileInput(): HTMLInputElement {
  const input = document.querySelector("input[type='file']");
  if (input === null) throw new Error("missing file input");
  return input as HTMLInputElement;
}

function png(name = "logo.png"): File {
  return new File(["bytes"], name, { type: "image/png" });
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  cleanup();
  resetAppClient();
  vi.unstubAllGlobals();
});

describe("WorkspaceLogoDialog", () => {
  it("refuses a wrong type and an oversize file inline", () => {
    stubApi();
    renderDialog();
    const gif = new File(["bytes"], "a.gif", { type: "image/gif" });
    fireEvent.change(fileInput(), { target: { files: [gif] } });
    expect(screen.getByRole("alert")).toHaveTextContent("Choose a JPEG, PNG or WebP image.");
    const big = new File([new Uint8Array(6 * 1024 * 1024)], "big.png", { type: "image/png" });
    fireEvent.change(fileInput(), { target: { files: [big] } });
    expect(screen.getByRole("alert")).toHaveTextContent("The image must be 5 MB or smaller.");
    expect(screen.getByRole("button", { name: "Upload & Save" })).toBeDisabled();
  });

  it("refuses a drop of the wrong type and accepts a valid one", async () => {
    stubApi();
    renderDialog();
    const zone = screen.getByText("Choose or drop an image").closest("label");
    if (zone === null) throw new Error("missing drop zone");
    fireEvent.dragOver(zone);
    expect(screen.getByText("Drop the image here")).toBeInTheDocument();
    fireEvent.dragLeave(zone);
    fireEvent.drop(zone, { dataTransfer: { files: [new File(["x"], "a.bmp", { type: "image/bmp" })] } });
    expect(screen.getByRole("alert")).toBeInTheDocument();
    fireEvent.drop(zone, { dataTransfer: { files: [png()] } });
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Upload & Save" })).toBeEnabled();
  });

  it("keeps Remove disabled without a logo and upload disabled without a file", () => {
    stubApi();
    renderDialog();
    expect(screen.getByRole("button", { name: "Remove" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Upload & Save" })).toBeDisabled();
  });

  it("uploads, closes with no notice and leaves the dialog clean for the next open", async () => {
    stubApi();
    const user = userEvent.setup();
    const { onOpenChange } = renderDialog();
    fireEvent.change(fileInput(), { target: { files: [png()] } });
    await user.click(screen.getByRole("button", { name: "Upload & Save" }));
    await waitFor(() => {
      expect(onOpenChange).toHaveBeenCalledWith(false);
    });
    expect(notifySuccess).not.toHaveBeenCalled();
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("keeps the dialog and toasts when the upload fails", async () => {
    stubApi({ grantStatus: 500 });
    const user = userEvent.setup();
    const { onOpenChange } = renderDialog();
    fireEvent.change(fileInput(), { target: { files: [png()] } });
    await user.click(screen.getByRole("button", { name: "Upload & Save" }));
    await waitFor(() => {
      expect(notifyError).toHaveBeenCalledWith("Error", expect.any(String));
    });
    expect(onOpenChange).not.toHaveBeenCalledWith(false);
  });

  it("removes a current asset, closes and toasts success", async () => {
    const { calls } = stubApi();
    const user = userEvent.setup();
    const { onOpenChange, queryClient } = renderDialog({ logoUrl: "/api/assets/v2/static/asset-9/" });
    queryClient.setQueryData(workspaceDetailsKey("acme"), {
      id: UUID,
      name: "Acme",
      slug: "acme",
      logo: "/api/assets/v2/static/asset-9/",
      logo_asset: UUID,
      logo_url: "/api/assets/v2/static/asset-9/",
    });
    await user.click(screen.getByRole("button", { name: "Remove" }));
    await waitFor(() => {
      expect(onOpenChange).toHaveBeenCalledWith(false);
    });
    expect(calls).toContain("DELETE https://api.test/api/assets/v2/workspaces/acme/asset-9/");
    expect(notifySuccess).toHaveBeenCalledWith("Success!", "Workspace picture removed successfully.");
    expect(queryClient.getQueryData(workspaceDetailsKey("acme"))).toMatchObject({
      logo: null,
      logo_asset: null,
      logo_url: null,
    });
  });

  it("removes a legacy full-URL logo through the file-asset path", async () => {
    const { calls } = stubApi();
    const user = userEvent.setup();
    renderDialog({ logoUrl: "https://cdn.test/old/logo.png" });
    await user.click(screen.getByRole("button", { name: "Remove" }));
    await waitFor(() => {
      expect(calls).toContain(`DELETE https://api.test/api/workspaces/file-assets/${UUID}/logo.png/`);
    });
  });

  it("previews a picked file through an object URL and revokes it when replaced", async () => {
    stubApi();
    let seq = 0;
    const create = vi.fn(() => `blob:preview-${++seq}`);
    const revoke = vi.fn();
    vi.stubGlobal("URL", { ...URL, createObjectURL: create, revokeObjectURL: revoke });
    try {
      renderDialog({ logoUrl: "/api/assets/v2/static/asset-9/" });
      fireEvent.change(fileInput(), { target: { files: [png()] } });
      await waitFor(() => {
        expect(create).toHaveBeenCalledTimes(1);
      });
      expect(document.querySelector("img[src='blob:preview-1']")).not.toBeNull();
      fireEvent.change(fileInput(), { target: { files: [png("second.png")] } });
      await waitFor(() => {
        expect(revoke).toHaveBeenCalledWith("blob:preview-1");
      });
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("skips the revoke when the environment cannot release object URLs", async () => {
    stubApi();
    vi.stubGlobal("URL", { createObjectURL: vi.fn(() => "blob:preview-1") });
    try {
      renderDialog();
      fireEvent.change(fileInput(), { target: { files: [png()] } });
      fireEvent.change(fileInput(), { target: { files: [png("second.png")] } });
      await waitFor(() => {
        expect(document.querySelector("img[src='blob:preview-1']")).not.toBeNull();
      });
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("keeps the stored image when the environment has no object-URL API", () => {
    stubApi();
    vi.stubGlobal("URL", { ...URL, createObjectURL: undefined, revokeObjectURL: vi.fn() });
    try {
      renderDialog({ logoUrl: "/api/assets/v2/static/asset-9/" });
      fireEvent.change(fileInput(), { target: { files: [png()] } });
      expect(document.querySelector("img[src='/api/assets/v2/static/asset-9/']")).not.toBeNull();
      expect(screen.getByRole("button", { name: "Upload & Save" })).toBeEnabled();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("clears the picked file when the system picker reports no selection", () => {
    stubApi();
    renderDialog();
    fireEvent.change(fileInput(), { target: { files: [png()] } });
    expect(screen.getByRole("button", { name: "Upload & Save" })).toBeEnabled();
    fireEvent.change(fileInput(), { target: { files: [] } });
    expect(screen.getByRole("button", { name: "Upload & Save" })).toBeDisabled();
  });

  it("clears the picked file when a drop carries no file", () => {
    stubApi();
    renderDialog();
    const zone = screen.getByText("Choose or drop an image").closest("label");
    if (zone === null) throw new Error("missing drop zone");
    fireEvent.drop(zone, { dataTransfer: { files: [png()] } });
    expect(screen.getByRole("button", { name: "Upload & Save" })).toBeEnabled();
    fireEvent.drop(zone, { dataTransfer: { files: [] } });
    expect(screen.getByRole("button", { name: "Upload & Save" })).toBeDisabled();
  });

  it("works without a picked file: Remove is a no-op and Upload stays inert", async () => {
    const { calls } = stubApi();
    const user = userEvent.setup();
    const { onOpenChange } = renderDialog();
    // Both controls are disabled; a forced click must still write nothing.
    const upload = screen.getByRole("button", { name: "Upload & Save" });
    upload.removeAttribute("disabled");
    fireEvent.click(upload);
    const remove = screen.getByRole("button", { name: "Remove" });
    await user.click(remove);
    expect(screen.queryByRole("button", { name: "Uploading" })).not.toBeInTheDocument();
    expect(calls.some((call) => call.startsWith("POST") || call.startsWith("DELETE"))).toBe(false);
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("starts one upload from two submits in the same frame", async () => {
    let releaseUpload: () => void = () => undefined;
    const uploadGate = new Promise<void>((resolve) => {
      releaseUpload = resolve;
    });
    stubApi();
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => {
        await uploadGate;
        return { ok: true, status: 204 };
      })
    );
    renderDialog({ logoUrl: "/api/assets/v2/static/asset-9/" });
    fireEvent.change(fileInput(), { target: { files: [png()] } });
    const upload = screen.getByRole("button", { name: "Upload & Save" });
    // Both clicks land in one frame, before React re-renders as "Uploading".
    act(() => {
      upload.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      upload.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    expect(await screen.findByRole("button", { name: "Uploading" })).toBeInTheDocument();
    // While uploading, a forced Remove must not start a removal.
    const remove = screen.getByRole("button", { name: "Remove" });
    remove.removeAttribute("disabled");
    fireEvent.click(remove);
    expect(screen.queryByRole("button", { name: "Removing" })).not.toBeInTheDocument();
    releaseUpload();
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: "Uploading" })).not.toBeInTheDocument();
    });
  });

  it("shows Removing and locks the upload while the removal runs", async () => {
    let release: () => void = () => undefined;
    const hold = new Promise<void>((resolve) => {
      release = resolve;
    });
    const calls: string[] = [];
    createAppClient("https://api.test", (async (url: string, init: { method?: string }) => {
      const method = init.method ?? "GET";
      calls.push(`${method} ${url}`);
      if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
      if (url.includes("/api/assets/v2/workspaces/acme/") && method === "DELETE") {
        await hold;
        return jsonResponse(url, 204, {});
      }
      if (url.endsWith("/api/workspaces/acme/") && method === "PATCH") {
        return jsonResponse(url, 200, {
          id: UUID,
          name: "Acme",
          slug: "acme",
          logo: null,
          logo_asset: null,
          logo_url: null,
          owner: UUID,
          organization_size: null,
          timezone: "UTC",
          background_color: "#fff",
          total_members: 1,
          created_by: null,
          updated_by: null,
          created_at: "2024-01-02T03:04:05Z",
          updated_at: "2024-02-03T04:05:06Z",
        });
      }
      throw new Error(`unexpected fetch ${method} ${url}`);
    }) as never);
    const { onOpenChange } = renderDialog({ logoUrl: "/api/assets/v2/static/asset-9/" });
    const remove = screen.getByRole("button", { name: "Remove" });
    // Two clicks in one frame both read the last render's `removing`, so the
    // handler's own ref must keep the second from issuing a DELETE.
    act(() => {
      remove.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      remove.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    await waitFor(() => {
      expect(calls).toContain("DELETE https://api.test/api/assets/v2/workspaces/acme/asset-9/");
    });
    expect(await screen.findByRole("button", { name: "Removing" })).toBeInTheDocument();
    // With a picked file, a forced Upload must not start beside Remove.
    fireEvent.change(fileInput(), { target: { files: [png()] } });
    const uploadButton = screen.getByRole("button", { name: "Upload & Save" });
    uploadButton.removeAttribute("disabled");
    act(() => {
      uploadButton.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    expect(screen.queryByRole("button", { name: "Uploading" })).not.toBeInTheDocument();
    expect(calls.filter((call) => call.startsWith("DELETE"))).toHaveLength(1);
    release();
    await waitFor(() => {
      expect(onOpenChange).toHaveBeenCalledWith(false);
    });
  });

  it("closes on Cancel and clears the picked file", async () => {
    stubApi();
    const user = userEvent.setup();
    const { onOpenChange } = renderDialog();
    fireEvent.change(fileInput(), { target: { files: [png()] } });
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("dismisses on Escape without writing", async () => {
    const { calls } = stubApi();
    const user = userEvent.setup();
    const { onOpenChange } = renderDialog({ logoUrl: "/api/assets/v2/static/asset-9/" });
    await user.keyboard("{Escape}");
    await waitFor(() => {
      expect(onOpenChange).toHaveBeenCalledWith(false);
    });
    expect(calls.some((call) => call.startsWith("DELETE"))).toBe(false);
  });
});
