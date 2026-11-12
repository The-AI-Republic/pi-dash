// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import type { OnboardingStep } from "../onboarding.js";
import { OnboardingShell } from "./OnboardingShell.js";

const { navigate } = vi.hoisted(() => ({ navigate: vi.fn() }));

vi.mock("@tanstack/react-router", () => ({ useNavigate: () => navigate }));

function renderShell(
  overrides: Partial<{
    step: OnboardingStep;
    progress: { current: number; total: number };
    canGoBack: boolean;
    accountName: string;
    accountAvatarUrl: string | null;
    accountEmail: string;
  }> = {}
) {
  const onBack = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const view = render(
    <QueryClientProvider client={queryClient}>
      <OnboardingShell
        step={overrides.step ?? "profile"}
        progress={overrides.progress ?? { current: 2, total: 6 }}
        canGoBack={overrides.canGoBack ?? true}
        onBack={onBack}
        accountName={overrides.accountName ?? "Ada Byron"}
        accountAvatarUrl={overrides.accountAvatarUrl ?? null}
        accountEmail={overrides.accountEmail ?? "ada@example.com"}
      >
        <p>Step content</p>
      </OnboardingShell>
    </QueryClientProvider>
  );
  return { onBack, view };
}

describe("OnboardingShell", () => {
  it("shows the progress fraction and the step content", () => {
    renderShell();
    const bar = screen.getByRole("progressbar", { name: "Onboarding progress" });
    expect(bar).toHaveAttribute("aria-valuenow", "2");
    expect(bar).toHaveAttribute("aria-valuemin", "1");
    expect(bar).toHaveAttribute("aria-valuemax", "6");
    expect(screen.getByText("2/6")).toBeInTheDocument();
    expect(screen.getByText("Step content")).toBeInTheDocument();
  });

  it("shows the account photo when one is stored", () => {
    const { view } = renderShell({ accountAvatarUrl: "https://cdn.test/ada.png" });
    expect(view.container.querySelector('img[src="https://cdn.test/ada.png"]')).not.toBeNull();
  });

  it("offers back except where there is no previous step", async () => {
    const user = userEvent.setup();
    const { onBack, view } = renderShell({ canGoBack: true });
    await user.click(screen.getByRole("button", { name: "Back" }));
    expect(onBack).toHaveBeenCalledTimes(1);

    view.rerender(
      <QueryClientProvider client={new QueryClient()}>
        <OnboardingShell
          step="profile"
          progress={{ current: 1, total: 6 }}
          canGoBack={false}
          onBack={onBack}
          accountName="Ada Byron"
          accountAvatarUrl={null}
          accountEmail="ada@example.com"
        >
          <p>Step content</p>
        </OnboardingShell>
      </QueryClientProvider>
    );
    expect(screen.queryByRole("button", { name: "Back" })).toBeNull();
  });

  it("opens the switch-account dialog from the account menu", () => {
    // fireEvent for the mouse path: back-to-back user-event clicks trip
    // the menu popup's mouseup guard under jsdom (see kit Menu tests).
    renderShell();
    fireEvent.click(screen.getByRole("button", { name: "Account: Ada Byron" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Use a different email address" }));
    expect(screen.getByRole("dialog", { name: "Switch accounts" })).toBeInTheDocument();
    expect(screen.getByText("ada@example.com")).toBeInTheDocument();
  });

  it("scrolls the step container to top on step change", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const renderAt = (step: OnboardingStep) =>
      render(
        <QueryClientProvider client={queryClient}>
          <OnboardingShell
            step={step}
            progress={{ current: 2, total: 6 }}
            canGoBack={true}
            onBack={() => undefined}
            accountName="Ada Byron"
            accountAvatarUrl={null}
            accountEmail="ada@example.com"
          >
            <p>Step content</p>
          </OnboardingShell>
        </QueryClientProvider>
      );
    const view = renderAt("profile");
    const scroller = screen.getByText("Step content").closest("div.overflow-y-auto");
    expect(scroller).not.toBeNull();
    if (scroller) scroller.scrollTop = 120;
    view.rerender(
      <QueryClientProvider client={queryClient}>
        <OnboardingShell
          step="role"
          progress={{ current: 3, total: 6 }}
          canGoBack={true}
          onBack={() => undefined}
          accountName="Ada Byron"
          accountAvatarUrl={null}
          accountEmail="ada@example.com"
        >
          <p>Step content</p>
        </OnboardingShell>
      </QueryClientProvider>
    );
    expect(screen.getByText("Step content").closest("div.overflow-y-auto")?.scrollTop).toBe(0);
  });

  it("keeps a zero progress bar at zero width", () => {
    renderShell({ progress: { current: 0, total: 0 } });
    const bar = screen.getByRole("progressbar", { name: "Onboarding progress" });
    expect(bar.firstElementChild).toHaveAttribute("style", "width: 0%;");
  });

  it("opens the account menu from the keyboard", async () => {
    const user = userEvent.setup();
    renderShell();
    screen.getByRole("button", { name: "Account: Ada Byron" }).focus();
    await user.keyboard("{ArrowDown}");
    expect(screen.getByRole("menuitem", { name: "Use a different email address" })).toHaveFocus();
  });
});
