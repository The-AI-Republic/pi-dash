// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const navigate = vi.fn();

// Stand-in for the router: navigation is TanStack's own tested behavior.
// What this suite owns is the shell composition plus the session-expired
// redirect decision.
vi.mock("@tanstack/react-router", () => ({
  useRouter: () => ({ state: { location: { href: "/acme/" } }, navigate }),
}));

import { createAppClient } from "../../core/api/index.js";
import { useCommandPalette } from "../commands/index.js";
import { useSessionStore } from "../../core/session/store.js";
import { AppShell } from "./AppShell.js";
import { useShortcutsDialog } from "./shortcutsDialogStore.js";
import { notifyError } from "./toaster.js";

function shell(slots: Partial<React.ComponentProps<typeof AppShell>> = {}): React.ReactElement {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return (
    <QueryClientProvider client={queryClient}>
      <AppShell
        topBar={<header>top bar</header>}
        sidebar={<aside aria-label="Workspace navigation">sidebar</aside>}
        searchScope={null}
        onSelectSearchResult={() => undefined}
        {...slots}
      >
        <p>page content</p>
      </AppShell>
    </QueryClientProvider>
  );
}

beforeEach(() => {
  createAppClient("https://api.test", async (url) => ({
    status: 200,
    url,
    headers: { get: () => null },
    text: async () => JSON.stringify({ results: {} }),
  }));
});

afterEach(() => {
  navigate.mockClear();
  useSessionStore.setState({ status: "unknown", returnUrl: null });
  useCommandPalette.getState().closePalette();
  useShortcutsDialog.getState().closeShortcuts();
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

  it("pads content while the rail stays suppressed and reclaims it with the rail (SHELL-099)", () => {
    const { unmount } = render(shell());
    expect(screen.getByRole("main").className).toContain("pl-(--space-4)");
    unmount();
    render(shell({ rail: <nav aria-label="App rail">rail</nav> }));
    expect(screen.getByRole("main").className).not.toContain("pl-(--space-4)");
  });

  it("toggles the palette on the mod+k shortcut", async () => {
    const user = userEvent.setup();
    render(shell());
    await user.keyboard("{Control>}k{/Control}");
    expect(await screen.findByRole("listbox", { name: "Commands" })).not.toBeNull();
  });

  it("opens the palette from inside a text field without navigating (SHELL-080)", async () => {
    const user = userEvent.setup();
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <AppShell
          topBar={<header>top bar</header>}
          sidebar={<aside aria-label="Workspace navigation">sidebar</aside>}
          searchScope={null}
          onSelectSearchResult={() => undefined}
        >
          <label>
            Note
            <input type="text" />
          </label>
        </AppShell>
      </QueryClientProvider>
    );
    const field = screen.getByLabelText("Note");
    await user.click(field);
    await user.type(field, "abc");
    await user.keyboard("{Control>}k{/Control}");
    const listbox = await screen.findByRole("listbox", { name: "Commands" });
    expect(listbox).not.toBeNull();
    expect(navigate).not.toHaveBeenCalled();
    expect((field as HTMLInputElement).value).toBe("abc");
  });

  it("ignores unmodified keys: no slash or bare-key entry (SHELL-080)", async () => {
    const user = userEvent.setup();
    render(shell());
    await user.keyboard("/");
    await user.keyboard("k");
    expect(screen.queryByRole("listbox", { name: "Commands" })).toBeNull();
  });

  it("keeps the sidebar slot mounted: the sidebar renders its own collapsed strip (SHELL-067)", () => {
    render(shell());
    // Collapse lives in the shell chrome store (SHELL-066/098) and the
    // Sidebar answers it with a zero-width strip; the slot never unmounts.
    expect(screen.getByRole("complementary", { name: "Workspace navigation" })).not.toBeNull();
  });

  it("mounts the shortcuts dialog closed (SHELL-094)", async () => {
    render(shell());
    expect(screen.queryByRole("dialog", { name: "Keyboard shortcuts" })).toBeNull();
    act(() => {
      useShortcutsDialog.getState().openShortcuts();
    });
    expect(await screen.findByRole("dialog", { name: "Keyboard shortcuts" })).not.toBeNull();
  });

  it("delivers a mount-time notice from page content: the host subscribes first (CLOUD-026)", async () => {
    // The kit manager emits to current subscribers only, so a notice
    // fired from a page mount effect is lost when the host sits after
    // the content. The Apps bounce (CLOUD-026) notifies exactly there.
    function NotifyingPage(): React.ReactElement {
      React.useEffect(() => {
        notifyError("Mount boom", "fired while the shell mounted");
      }, []);
      return <p>page content</p>;
    }
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <AppShell
          topBar={<header>top bar</header>}
          sidebar={<aside aria-label="Workspace navigation">sidebar</aside>}
          searchScope={null}
          onSelectSearchResult={() => undefined}
        >
          <NotifyingPage />
        </AppShell>
      </QueryClientProvider>
    );
    expect(await screen.findByText("Mount boom")).not.toBeNull();
  });

  it("redirects to sign-in once when the session expires", () => {
    render(shell());
    expect(navigate).not.toHaveBeenCalled();
    act(() => {
      useSessionStore.setState({ status: "expired", returnUrl: "/acme/" });
    });
    // next_path, not next: the spec and the AUTH-020 oracle pin next_path
    // in the expired-bounce URL (session-account.spec.ts).
    expect(navigate).toHaveBeenCalledWith({ to: "/", search: { next_path: "/acme/" } });
  });
});
