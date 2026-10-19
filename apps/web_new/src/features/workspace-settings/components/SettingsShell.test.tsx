// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shell composition (WSET-001/002/007/039/040, spec §1/§2/§39/§40): the
// frame renders the identity, grouped entries, the section heading and
// the registered body — and nothing that would let settings drag, import
// files or behave differently on desktop.
import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { ROLE_ADMIN, ROLE_MEMBER, selectPermissions } from "../../../core/session/index.js";
import { SettingsShell } from "./SettingsShell.js";

afterEach(() => {
  document.title = "";
});

function renderShell(overrides: Partial<React.ComponentProps<typeof SettingsShell>> = {}) {
  const onNavigate = vi.fn();
  const props: React.ComponentProps<typeof SettingsShell> = {
    workspaceSlug: "acme",
    workspaceName: "Acme",
    logoUrl: null,
    role: ROLE_ADMIN,
    permissions: selectPermissions(ROLE_ADMIN),
    section: "members",
    webhookDetail: false,
    onNavigate,
    body: <p>section body</p>,
    ...overrides,
  };
  const view = render(<SettingsShell {...props} />);
  return {
    onNavigate,
    container: view.container,
    rerender: (next: Partial<React.ComponentProps<typeof SettingsShell>>) =>
      view.rerender(<SettingsShell {...props} {...next} />),
  };
}

describe("SettingsShell", () => {
  it("renders the identity, the grouped entries and the section heading", () => {
    renderShell();
    expect(screen.getByTestId("workspace-settings-sidebar")).toBeInTheDocument();
    expect(screen.getByText("Acme")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Members", level: 1 })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Members" })).toHaveAttribute("aria-current", "page");
  });

  it("titles the tab with the workspace and section", () => {
    const { rerender } = renderShell();
    expect(document.title).toBe("Acme - Members");
    rerender({ section: "billing" });
    expect(document.title).toBe("Acme - Billing & Plans");
  });

  it("titles a webhook detail address and heads the frame with Webhook", () => {
    renderShell({ section: "webhooks", webhookDetail: true });
    expect(document.title).toBe("Acme - Webhook");
    expect(screen.getByRole("heading", { name: "Webhook", level: 1 })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Webhooks" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByTestId("workspace-settings-mobile-crumb")).toHaveTextContent("Webhook");
  });

  it("renders the section body inside the frame", () => {
    renderShell({ body: <p>roster</p> });
    expect(screen.getByText("roster")).toBeInTheDocument();
  });

  it("filters the entries by role", () => {
    renderShell({ role: ROLE_MEMBER, permissions: selectPermissions(ROLE_MEMBER), section: "general" });
    expect(screen.queryByRole("link", { name: "Billing & Plans" })).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Developer" })).not.toBeInTheDocument();
  });

  it("returns to the workspace home from the back control", () => {
    const { onNavigate } = renderShell();
    screen.getByRole("link", { name: /Back to workspace/ }).click();
    expect(onNavigate).toHaveBeenCalledWith("/acme");
  });

  it("renders no placeholder of its own", () => {
    renderShell();
    expect(screen.queryByTestId("workspace-settings-section-placeholder")).not.toBeInTheDocument();
  });

  it("carries no drag targets, file pickers, import controls or desktop hooks (§39/§40)", () => {
    const { container } = renderShell();
    expect(container.querySelectorAll("[draggable='true'], input[type='file'], [data-desktop-only]")).toHaveLength(0);
    expect(container.querySelectorAll('[data-testid^="desktop-"]')).toHaveLength(0);
    expect(screen.queryByText(/import/i)).not.toBeInTheDocument();
  });
});
