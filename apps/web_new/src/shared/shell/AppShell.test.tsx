// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const navigate = vi.fn();

// Stand-in for the router: navigation is TanStack's own tested behavior.
// What this suite owns is the shell composition plus the session-expired
// redirect decision.
vi.mock("@tanstack/react-router", () => ({
  useRouter: () => ({ state: { location: { href: "/acme/" } }, navigate }),
}));

import { useCommandPalette } from "../commands/index.js";
import { useSessionStore } from "../../core/session/store.js";
import { AppShell } from "./AppShell.js";

function shell(slots: Partial<React.ComponentProps<typeof AppShell>> = {}): React.ReactElement {
  return (
    <AppShell
      topBar={<header>top bar</header>}
      sidebar={<aside aria-label="Workspace navigation">sidebar</aside>}
      {...slots}
    >
      <p>page content</p>
    </AppShell>
  );
}

afterEach(() => {
  navigate.mockClear();
  useSessionStore.setState({ status: "unknown", returnUrl: null });
  useCommandPalette.getState().closePalette();
});

describe("AppShell", () => {
  it("renders chrome, content and the global hosts", () => {
    render(shell());
    expect(screen.getByText("top bar")).not.toBeNull();
    expect(screen.getByRole("complementary", { name: "Workspace navigation" })).not.toBeNull();
    expect(screen.getByText("page content")).not.toBeNull();
    // Palette shortcut opens the command palette.
    expect(screen.queryByRole("listbox", { name: "Commands" })).toBeNull();
  });

  it("mounts the rail, slide-over and dialog slots when provided", () => {
    render(
      shell({
        rail: <nav aria-label="App rail">rail</nav>,
        slideOver: <section aria-label="Overflow">overflow list</section>,
        dialogs: (
          <div role="dialog" aria-label="Global dialog">
            dialog
          </div>
        ),
      })
    );
    expect(screen.getByRole("navigation", { name: "App rail" })).not.toBeNull();
    expect(screen.getByRole("complementary", { name: "Slide-over" })).not.toBeNull();
    expect(screen.getByText("overflow list")).not.toBeNull();
    expect(screen.getByRole("dialog", { name: "Global dialog" })).not.toBeNull();
  });

  it("omits the slide-over landmark when the slot is empty", () => {
    render(shell());
    expect(screen.queryByRole("complementary", { name: "Slide-over" })).toBeNull();
  });

  it("toggles the palette on the mod+k shortcut", async () => {
    const user = userEvent.setup();
    render(shell());
    await user.keyboard("{Control>}k{/Control}");
    expect(await screen.findByRole("listbox", { name: "Commands" })).not.toBeNull();
  });

  it("redirects to sign-in once when the session expires", () => {
    render(shell());
    expect(navigate).not.toHaveBeenCalled();
    act(() => {
      useSessionStore.setState({ status: "expired", returnUrl: "/acme/" });
    });
    expect(navigate).toHaveBeenCalledWith({ to: "/sign-in", search: { next: "/acme/" } });
  });
});
