// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One workspace row of the account navigator (PROF-009; main#675): the
// current workspace starts open, the others start closed, a workspace
// with nothing to open is a plain way home, entries follow that
// workspace's role, and only the viewed workspace's entry reads current.
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER } from "../../../../core/session/index.js";
import { WorkspaceSettingsRow, workspaceInitial } from "./WorkspaceSettingsRow.js";

const ACME = { id: "ws-1", slug: "acme", name: "Acme", role: ROLE_MEMBER };
const GLOBEX = { id: "ws-2", slug: "globex", name: "globex studio", role: ROLE_ADMIN };
const LEDGER = { id: "ws-3", slug: "ledger", name: "Ledger", role: ROLE_GUEST };

function setup({
  workspace = ACME,
  initiallyExpanded = false,
  pathname = "/acme/settings/account/general/",
}: {
  workspace?: typeof ACME;
  initiallyExpanded?: boolean;
  pathname?: string;
} = {}) {
  const onNavigate = vi.fn();
  render(
    <ul>
      <WorkspaceSettingsRow
        workspace={workspace}
        initiallyExpanded={initiallyExpanded}
        pathname={pathname}
        onNavigate={onNavigate}
      />
    </ul>
  );
  return { onNavigate };
}

describe("WorkspaceSettingsRow", () => {
  it("starts open for the workspace being viewed, with its permitted entries", () => {
    setup({ initiallyExpanded: true });
    const toggle = screen.getByTestId("account-settings-workspace-toggle-acme");
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByTestId("account-settings-workspace-group-acme")).toBeInTheDocument();
    expect(screen.getByTestId("account-settings-workspace-link-acme-general")).toHaveAttribute(
      "href",
      "/acme/settings/"
    );
    expect(screen.getByTestId("account-settings-workspace-link-acme-members")).toHaveAttribute(
      "href",
      "/acme/settings/members/"
    );
    // A member sees neither the admin-only entries nor the Developer group.
    expect(screen.queryByTestId("account-settings-workspace-link-acme-billing")).not.toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Acme Developer" })).not.toBeInTheDocument();
    const administration = screen.getByRole("region", { name: "Acme Administration" });
    expect(administration).toContainElement(screen.getByTestId("account-settings-workspace-link-acme-members"));
  });

  it("starts closed for another workspace and opens on demand", () => {
    setup({ workspace: GLOBEX });
    const toggle = screen.getByTestId("account-settings-workspace-toggle-globex");
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByTestId("account-settings-workspace-group-globex")).not.toBeInTheDocument();
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByTestId("account-settings-workspace-group-globex")).toBeInTheDocument();
    // An admin there sees the admin-only entries, grouped by that catalog.
    expect(screen.getByTestId("account-settings-workspace-link-globex-billing")).toHaveAttribute(
      "href",
      "/globex/settings/billing/"
    );
    expect(screen.getByRole("region", { name: "globex studio Developer" })).toBeInTheDocument();
    fireEvent.click(toggle);
    expect(screen.queryByTestId("account-settings-workspace-group-globex")).not.toBeInTheDocument();
  });

  it("uses that workspace's own role, not the one being viewed", () => {
    setup({ workspace: LEDGER, initiallyExpanded: true });
    const row = screen.getByTestId("account-settings-workspace-ledger");
    expect(row).toHaveAttribute("data-expandable", "false");
    expect(screen.queryByTestId("account-settings-workspace-toggle-ledger")).not.toBeInTheDocument();
    expect(screen.getByTestId("account-settings-workspace-home-ledger")).toHaveAttribute("href", "/ledger/");
    // The plain row links home instead of listing entries, so an entry
    // selector scoped to the row finds nothing (the parity driver relies
    // on this: the guest row must read as an empty list of pages).
    expect(row.querySelectorAll('[data-testid^="account-settings-workspace-link-"]')).toHaveLength(0);
  });

  it("highlights only the entry of the workspace being viewed", () => {
    const pathname = "/globex/settings/webhooks/w-1";
    render(
      <ul>
        <WorkspaceSettingsRow workspace={ACME} initiallyExpanded pathname={pathname} onNavigate={vi.fn()} />
      </ul>
    );
    expect(screen.getByTestId("account-settings-workspace-link-acme-general")).not.toHaveAttribute("aria-current");
    render(
      <ul>
        <WorkspaceSettingsRow workspace={GLOBEX} initiallyExpanded pathname={pathname} onNavigate={vi.fn()} />
      </ul>
    );
    expect(screen.getByTestId("account-settings-workspace-link-globex-webhooks")).toHaveAttribute(
      "aria-current",
      "page"
    );
  });

  it("lights nothing while account settings are open", () => {
    setup({ pathname: "/globex/settings/account/security/", initiallyExpanded: true });
    expect(screen.getByTestId("account-settings-workspace-link-acme-general")).not.toHaveAttribute("aria-current");
  });

  it("navigates in-app on a plain click and leaves modified clicks alone", () => {
    const { onNavigate } = setup({ initiallyExpanded: true });
    fireEvent.click(screen.getByTestId("account-settings-workspace-link-acme-members"));
    expect(onNavigate).toHaveBeenCalledWith("/acme/settings/members/");
    onNavigate.mockClear();
    for (const modifier of [{ metaKey: true }, { ctrlKey: true }, { shiftKey: true }, { altKey: true }]) {
      fireEvent.click(screen.getByTestId("account-settings-workspace-link-acme-members"), modifier);
    }
    expect(onNavigate).not.toHaveBeenCalled();
  });

  it("navigates home from a row with no expander, in-app and through the browser", () => {
    const { onNavigate } = setup({ workspace: LEDGER });
    const link = screen.getByTestId("account-settings-workspace-home-ledger");
    fireEvent.click(link);
    expect(onNavigate).toHaveBeenCalledWith("/ledger/");
    onNavigate.mockClear();
    fireEvent.click(link, { metaKey: true });
    expect(onNavigate).not.toHaveBeenCalled();
  });

  it("shows the workspace initial, or a question mark for a blank name", () => {
    expect(workspaceInitial("acme")).toBe("A");
    expect(workspaceInitial("  globex")).toBe("G");
    expect(workspaceInitial("   ")).toBe("?");
  });
});
