// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { OpenhubApp } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, type Mock, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { APP_INSTALLED_BADGE, APP_NEEDS_CONNECTION_BADGE, AppCard } from "./AppCard.js";

const TINY_SVG = '<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect width="32" height="32"/></svg>';

function card(overrides: Partial<OpenhubApp> = {}): OpenhubApp {
  return { appId: "app-fresh", slug: "fresh", name: "Fresh App", ...overrides };
}

function handlers(): {
  onInstall: Mock<(app: OpenhubApp) => Promise<void>>;
  onConnect: Mock<(app: OpenhubApp) => Promise<void>>;
  onUninstall: Mock<(app: OpenhubApp) => Promise<void>>;
} {
  return { onInstall: vi.fn(async () => {}), onConnect: vi.fn(async () => {}), onUninstall: vi.fn(async () => {}) };
}

function renderCard(app: OpenhubApp, mocked = handlers()) {
  return {
    ...render(
      <ul>
        <AppCard app={app} onInstall={mocked.onInstall} onConnect={mocked.onConnect} onUninstall={mocked.onUninstall} />
      </ul>
    ),
    mocked,
  };
}

beforeEach(() => {
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => {
  resetAppClient();
});

describe("AppCard", () => {
  it("renders the app identity (CLOUD-022)", () => {
    renderCard(card());
    const node = screen.getByTestId("app-card");
    expect(node).toHaveAttribute("data-app-id", "app-fresh");
    expect(screen.getByTestId("app-card-name")).toHaveTextContent("Fresh App");
  });

  it("shows the description when the row carries one", () => {
    renderCard(card({ description: "Fresh App description" }));
    expect(screen.getByText("Fresh App description")).toBeInTheDocument();
  });

  it("skips the description when the row carries none", () => {
    renderCard(card({ description: undefined }));
    expect(screen.queryByText("Fresh App description")).not.toBeInTheDocument();
    const { unmount } = render(
      <ul>
        <AppCard
          app={card({ description: "" })}
          onInstall={async () => {}}
          onConnect={async () => {}}
          onUninstall={async () => {}}
        />
      </ul>
    );
    expect(screen.getAllByTestId("app-card")).toHaveLength(2);
    unmount();
  });

  it("marks installed and unfinished cards, and leaves fresh ones bare (CLOUD-028)", () => {
    const { rerender } = render(
      <ul>
        <AppCard app={card()} onInstall={async () => {}} onConnect={async () => {}} onUninstall={async () => {}} />
      </ul>
    );
    expect(screen.queryByText(APP_INSTALLED_BADGE)).not.toBeInTheDocument();
    expect(screen.queryByText(APP_NEEDS_CONNECTION_BADGE)).not.toBeInTheDocument();

    rerender(
      <ul>
        <AppCard
          app={card({ isInstalled: true, suggestedAction: "none" })}
          onInstall={async () => {}}
          onConnect={async () => {}}
          onUninstall={async () => {}}
        />
      </ul>
    );
    expect(screen.getByText(APP_INSTALLED_BADGE)).toBeInTheDocument();

    rerender(
      <ul>
        <AppCard
          app={card({ isInstalled: true, suggestedAction: "connect_auth" })}
          onInstall={async () => {}}
          onConnect={async () => {}}
          onUninstall={async () => {}}
        />
      </ul>
    );
    expect(screen.getByText(APP_NEEDS_CONNECTION_BADGE)).toBeInTheDocument();
    expect(screen.queryByText(APP_INSTALLED_BADGE)).not.toBeInTheDocument();
  });

  it("offers Install, Connect+Uninstall, or Uninstall by state (CLOUD-028)", () => {
    const { rerender, mocked } = renderCard(card());
    expect(screen.getByRole("button", { name: "Install" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Connect" })).not.toBeInTheDocument();

    rerender(
      <ul>
        <AppCard
          app={card({ isInstalled: true, suggestedAction: "connect_auth" })}
          onInstall={mocked.onInstall}
          onConnect={mocked.onConnect}
          onUninstall={mocked.onUninstall}
        />
      </ul>
    );
    expect(screen.getByRole("button", { name: "Connect" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Uninstall" })).toBeInTheDocument();

    rerender(
      <ul>
        <AppCard
          app={card({ isInstalled: true, suggestedAction: "none" })}
          onInstall={mocked.onInstall}
          onConnect={mocked.onConnect}
          onUninstall={mocked.onUninstall}
        />
      </ul>
    );
    expect(screen.queryByRole("button", { name: "Install" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Connect" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Uninstall" })).toBeInTheDocument();
  });

  it("routes each action to its handler with the card's app (CLOUD-024/028)", async () => {
    const pending = card({ appId: "app-pending", isInstalled: true, suggestedAction: "connect_auth" });
    const { mocked } = renderCard(pending);

    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    await waitFor(() => expect(mocked.onConnect).toHaveBeenCalledWith(pending));
    await waitFor(() => expect(screen.getByRole("button", { name: "Uninstall" })).not.toBeDisabled());
    fireEvent.click(screen.getByRole("button", { name: "Uninstall" }));
    await waitFor(() => expect(mocked.onUninstall).toHaveBeenCalledWith(pending));

    const { unmount } = render(
      <ul>
        <AppCard
          app={card()}
          onInstall={mocked.onInstall}
          onConnect={mocked.onConnect}
          onUninstall={mocked.onUninstall}
        />
      </ul>
    );
    fireEvent.click(screen.getAllByRole("button", { name: "Install" })[0] as HTMLElement);
    await waitFor(() => expect(mocked.onInstall).toHaveBeenCalled());
    unmount();
  });

  it("guards double clicks with a per-card busy state (CLOUD-024/028)", async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const onInstall = vi.fn(() => gate);
    render(
      <ul>
        <AppCard app={card()} onInstall={onInstall} onConnect={async () => {}} onUninstall={async () => {}} />
      </ul>
    );

    const button = screen.getByRole("button", { name: "Install" });
    fireEvent.click(button);
    await waitFor(() => expect(button).toBeDisabled());
    fireEvent.click(button);
    expect(onInstall).toHaveBeenCalledTimes(1);

    release();
    await waitFor(() => expect(button).not.toBeDisabled());
  });

  it("keeps the neighbor live while one card works (CLOUD-028)", async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    render(
      <ul>
        <AppCard
          app={card({ appId: "app-slow", name: "Slow App" })}
          onInstall={() => gate}
          onConnect={async () => {}}
          onUninstall={async () => {}}
        />
        <AppCard
          app={card({ appId: "app-other", name: "Other App" })}
          onInstall={async () => {}}
          onConnect={async () => {}}
          onUninstall={async () => {}}
        />
      </ul>
    );

    const [slow, other] = screen.getAllByRole("button", { name: "Install" });
    fireEvent.click(slow as HTMLElement);
    await waitFor(() => expect(slow).toBeDisabled());
    expect(other).not.toBeDisabled();

    release();
    await waitFor(() => expect(slow).not.toBeDisabled());
  });

  it("prefers the self-hosted graphic, then the proxy, then the tile (CLOUD-028)", () => {
    const { rerender } = render(
      <ul>
        <AppCard
          app={card({ iconSvgFallback: TINY_SVG, iconUrl: "https://cdn.example.com/ignored.png" })}
          onInstall={async () => {}}
          onConnect={async () => {}}
          onUninstall={async () => {}}
        />
      </ul>
    );
    const graphic = screen.getByTestId("app-card-icon");
    expect(graphic.getAttribute("src")?.startsWith("data:image/svg+xml,")).toBe(true);
    expect(screen.queryByTestId("app-card-tile")).not.toBeInTheDocument();

    rerender(
      <ul>
        <AppCard
          app={card({ appId: "app-proxied", iconUrl: "https://cdn.example.com/icon.png" })}
          onInstall={async () => {}}
          onConnect={async () => {}}
          onUninstall={async () => {}}
        />
      </ul>
    );
    expect(screen.getByTestId("app-card-icon")).toHaveAttribute(
      "src",
      "https://api.test/api/openhub/apps/icon/?url=https%3A%2F%2Fcdn.example.com%2Ficon.png"
    );

    rerender(
      <ul>
        <AppCard
          app={card({ appId: "app-tile" })}
          onInstall={async () => {}}
          onConnect={async () => {}}
          onUninstall={async () => {}}
        />
      </ul>
    );
    expect(screen.getByTestId("app-card-tile")).toHaveTextContent("F");
    expect(screen.queryByTestId("app-card-icon")).not.toBeInTheDocument();
  });

  it("steps down the icon chain on load failures (CLOUD-028)", () => {
    render(
      <ul>
        <AppCard
          app={card({ iconSvgFallback: TINY_SVG, iconUrl: "https://cdn.example.com/icon.png" })}
          onInstall={async () => {}}
          onConnect={async () => {}}
          onUninstall={async () => {}}
        />
      </ul>
    );
    fireEvent.error(screen.getByTestId("app-card-icon"));
    expect(screen.getByTestId("app-card-icon")).toHaveAttribute(
      "src",
      "https://api.test/api/openhub/apps/icon/?url=https%3A%2F%2Fcdn.example.com%2Ficon.png"
    );
    fireEvent.error(screen.getByTestId("app-card-icon"));
    expect(screen.getByTestId("app-card-tile")).toHaveTextContent("F");
  });

  it("restarts the icon chain when the slot renders a new row", () => {
    const { rerender } = render(
      <ul>
        <AppCard
          app={card({ appId: "app-broken", iconUrl: "https://cdn.example.com/missing.png" })}
          onInstall={async () => {}}
          onConnect={async () => {}}
          onUninstall={async () => {}}
        />
      </ul>
    );
    fireEvent.error(screen.getByTestId("app-card-icon"));
    expect(screen.getByTestId("app-card-tile")).toBeInTheDocument();

    rerender(
      <ul>
        <AppCard
          app={card({ appId: "app-inline", iconSvgFallback: TINY_SVG })}
          onInstall={async () => {}}
          onConnect={async () => {}}
          onUninstall={async () => {}}
        />
      </ul>
    );
    expect(screen.getByTestId("app-card-icon").getAttribute("src")?.startsWith("data:image/svg+xml,")).toBe(true);
  });
});
