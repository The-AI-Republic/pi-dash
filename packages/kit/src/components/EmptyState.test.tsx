// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { EmptyState, emptyStateImagePath } from "./EmptyState";

const ART = { category: "general", type: "empty" };

describe("emptyStateImagePath", () => {
  it("resolves the light path by default", () => {
    expect(emptyStateImagePath(ART)).toBe("/empty-state/general/empty-light.svg");
  });

  it("resolves the explicit light path", () => {
    expect(emptyStateImagePath(ART, "light")).toBe("/empty-state/general/empty-light.svg");
  });

  it("resolves the dark path for the dark scheme", () => {
    expect(emptyStateImagePath(ART, "dark")).toBe("/empty-state/general/empty-dark.svg");
  });

  it("resolves the dark path for the high-contrast scheme", () => {
    expect(emptyStateImagePath(ART, "high-contrast")).toBe("/empty-state/general/empty-dark.svg");
  });
});

describe("illustrated tier", () => {
  it("centers art with a heading and themed image", () => {
    render(<EmptyState variant="illustrated" title="Nothing here" description="Try again later" art={ART} />);
    const heading = screen.getByRole("heading", { level: 3 });
    expect(heading).toHaveTextContent("Nothing here");
    expect(heading).toHaveTextContent("Try again later");
    const img = heading.parentElement?.querySelector("img");
    expect(img?.getAttribute("src")).toBe("/empty-state/general/empty-light.svg");
    expect(img?.getAttribute("alt")).toBe("");
    expect(img?.getAttribute("aria-hidden")).toBe("true");
    expect(img?.getAttribute("draggable")).toBe("false");
    expect(img?.getAttribute("title")).toBe("Nothing here");
  });

  it("renders without a description", () => {
    render(<EmptyState variant="illustrated" title="Nothing here" art={ART} />);
    expect(screen.getByRole("heading", { level: 3 })).toHaveTextContent("Nothing here");
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("uses the body text tokens for the title and description", () => {
    const { container } = render(
      <EmptyState variant="illustrated" title="Nothing here" description="body copy" art={ART} />
    );
    const spans = container.querySelectorAll("h3 > span");
    expect(spans[0]?.className).toContain("text-(--text)");
    expect(spans[1]?.className).toContain("text-(--text-muted)");
  });

  it("resolves dark art under the dark scheme", () => {
    const { container } = render(<EmptyState variant="illustrated" title="Nothing here" art={ART} scheme="dark" />);
    expect(container.querySelector("img")?.getAttribute("src")).toBe("/empty-state/general/empty-dark.svg");
  });
});

describe("artwork node branch", () => {
  it("renders a custom element inline instead of an img", () => {
    const { container } = render(
      <EmptyState variant="illustrated" title="Custom" art={<svg data-testid="inline-art" />} />
    );
    expect(container.querySelector("img")).toBeNull();
    expect(screen.getByTestId("inline-art")).not.toBeNull();
  });

  it("renders node children that are not elements", () => {
    const { container } = render(<EmptyState variant="illustrated" title="Kids" art={[]} />);
    expect(container.querySelector("img")).toBeNull();
  });

  it("renders text art inline", () => {
    const { container } = render(<EmptyState variant="illustrated" title="Words" art="inline words" />);
    expect(container.querySelector("img")).toBeNull();
    expect(container.textContent).toContain("inline words");
  });

  it("renders nothing for null art", () => {
    const { container } = render(<EmptyState variant="illustrated" title="Bare" art={null} />);
    expect(container.querySelector("img")).toBeNull();
    expect(screen.getByRole("heading", { level: 3 })).toHaveTextContent("Bare");
  });

  it("renders nothing for undefined art", () => {
    const { container } = render(<EmptyState variant="illustrated" title="Bare" art={undefined} />);
    expect(container.querySelector("img")).toBeNull();
    expect(screen.getByRole("heading", { level: 3 })).toHaveTextContent("Bare");
  });
});

describe("marketing tier", () => {
  it("renders wide art, heading, and the primary action", () => {
    const onSelect = vi.fn();
    render(
      <EmptyState
        variant="marketing"
        title="Welcome aboard"
        description="Get started"
        art={ART}
        primaryAction={{ label: "Start", onSelect }}
      />
    );
    expect(document.querySelector("img")?.getAttribute("src")).toContain("empty-light.svg");
    fireEvent.click(screen.getByRole("button", { name: "Start" }));
    expect(onSelect).toHaveBeenCalledTimes(1);
  });

  it("renders without a description or action", () => {
    render(<EmptyState variant="marketing" title="Welcome aboard" art={ART} />);
    expect(screen.getByRole("heading", { level: 3 })).toHaveTextContent("Welcome aboard");
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("shows the hover hint on the primary action", () => {
    render(
      <EmptyState
        variant="marketing"
        title="Welcome aboard"
        art={ART}
        primaryAction={{ label: "Start", onSelect: () => {} }}
        hint="Begins the tour"
      />
    );
    expect(screen.getByRole("button", { name: "Start" }).getAttribute("title")).toBe("Begins the tour");
  });

  it("passes disabled and loading through to the primary action", () => {
    render(
      <EmptyState
        variant="marketing"
        title="Welcome aboard"
        art={ART}
        primaryAction={{ label: "Start", onSelect: () => {}, disabled: true, loading: true }}
      />
    );
    const button = screen.getByRole("button", { name: "Start" });
    expect(button.hasAttribute("disabled")).toBe(true);
    expect(button.getAttribute("aria-busy")).toBe("true");
  });
});

describe("detailed tier", () => {
  it("renders text-first with primary and secondary actions", () => {
    const primary = vi.fn();
    const secondary = vi.fn();
    const { container } = render(
      <EmptyState
        variant="detailed"
        title="Feature off"
        description="Turn it on"
        art={ART}
        primaryAction={{ label: "Enable", onSelect: primary }}
        secondaryAction={{ label: "Learn more", onSelect: secondary }}
      />
    );
    const order = Array.from(container.firstElementChild!.children).map((child) => child.tagName);
    expect(order).toEqual(["H3", "IMG", "DIV"]);
    fireEvent.click(screen.getByRole("button", { name: "Enable" }));
    fireEvent.click(screen.getByRole("button", { name: "Learn more" }));
    expect(primary).toHaveBeenCalledTimes(1);
    expect(secondary).toHaveBeenCalledTimes(1);
  });

  it("renders a primary action alone", () => {
    render(
      <EmptyState
        variant="detailed"
        title="Feature off"
        art={ART}
        primaryAction={{ label: "Enable", onSelect: () => {} }}
      />
    );
    expect(screen.getByRole("button", { name: "Enable" })).not.toBeNull();
    expect(screen.queryByRole("button", { name: "Learn more" })).toBeNull();
  });

  it("renders a secondary action alone", () => {
    render(
      <EmptyState
        variant="detailed"
        title="Feature off"
        art={ART}
        secondaryAction={{ label: "Learn more", onSelect: () => {} }}
      />
    );
    expect(screen.queryByRole("button", { name: "Enable" })).toBeNull();
    expect(screen.getByRole("button", { name: "Learn more" })).not.toBeNull();
  });

  it("renders without actions", () => {
    render(<EmptyState variant="detailed" title="Feature off" art={ART} />);
    expect(screen.queryByRole("button")).toBeNull();
  });

  it.each([
    ["small", "max-w-60"],
    ["base", "max-w-96"],
    ["large", "max-w-xl"],
  ] as const)("caps the %s size", (size, width) => {
    const { container } = render(<EmptyState variant="detailed" title="Feature off" art={ART} size={size} />);
    expect(container.firstElementChild?.className).toContain(width);
  });

  it("defaults to the base width", () => {
    const { container } = render(<EmptyState variant="detailed" title="Feature off" art={ART} />);
    expect(container.firstElementChild?.className).toContain("max-w-96");
  });
});

describe("section tier", () => {
  it("renders the compact box with icon, title, description, and action", () => {
    const onSelect = vi.fn();
    render(
      <EmptyState
        variant="section"
        title="Updates paused"
        description="Resume any time"
        icon={<span data-testid="section-icon">i</span>}
        action={{ label: "Resume", onSelect }}
      />
    );
    expect(screen.getByTestId("section-icon")).not.toBeNull();
    expect(screen.queryByRole("heading")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Resume" }));
    expect(onSelect).toHaveBeenCalledTimes(1);
  });

  it("renders without icon, description, or action", () => {
    const { container } = render(<EmptyState variant="section" title="Quiet" />);
    expect(container.textContent).toContain("Quiet");
    expect(screen.queryByRole("button")).toBeNull();
    expect(container.querySelector("img")).toBeNull();
  });

  it("keeps the title a plain span and the description on the muted token", () => {
    render(<EmptyState variant="section" title="Quiet" description="muted copy" />);
    const title = screen.getByText("Quiet");
    expect(title.tagName).toBe("SPAN");
    expect(title.className).toContain("text-(--text)");
    const description = screen.getByText("muted copy").parentElement;
    expect(description?.className).toContain("text-(--text-muted)");
  });
});

describe("document scheme default", () => {
  afterEach(() => {
    document.documentElement.removeAttribute("data-theme");
  });

  it("resolves dark art when the document mark is dark", () => {
    document.documentElement.setAttribute("data-theme", "dark");
    const { container } = render(<EmptyState variant="illustrated" title="Nothing here" art={ART} />);
    expect(container.querySelector("img")?.getAttribute("src")).toBe("/empty-state/general/empty-dark.svg");
  });

  it("shares the dark pair with dark-contrast and the light pair with light-contrast", () => {
    document.documentElement.setAttribute("data-theme", "dark-contrast");
    const dark = render(<EmptyState variant="illustrated" title="Nothing here" art={ART} />);
    expect(dark.container.querySelector("img")?.getAttribute("src")).toBe("/empty-state/general/empty-dark.svg");
    dark.unmount();
    document.documentElement.setAttribute("data-theme", "light-contrast");
    const light = render(<EmptyState variant="illustrated" title="Nothing here" art={ART} />);
    expect(light.container.querySelector("img")?.getAttribute("src")).toBe("/empty-state/general/empty-light.svg");
  });

  it("falls back to light for custom, missing and unknown marks", () => {
    for (const mark of ["custom", "sepia"]) {
      document.documentElement.setAttribute("data-theme", mark);
      const { container, unmount } = render(<EmptyState variant="illustrated" title="Nothing here" art={ART} />);
      expect(container.querySelector("img")?.getAttribute("src")).toBe("/empty-state/general/empty-light.svg");
      unmount();
    }
    document.documentElement.removeAttribute("data-theme");
    const { container } = render(<EmptyState variant="illustrated" title="Nothing here" art={ART} />);
    expect(container.querySelector("img")?.getAttribute("src")).toBe("/empty-state/general/empty-light.svg");
  });

  it("lets an explicit scheme win over the document mark", () => {
    document.documentElement.setAttribute("data-theme", "dark");
    const { container } = render(<EmptyState variant="illustrated" title="Nothing here" art={ART} scheme="light" />);
    expect(container.querySelector("img")?.getAttribute("src")).toBe("/empty-state/general/empty-light.svg");
  });

  it("follows live theme switches without a reload", async () => {
    document.documentElement.removeAttribute("data-theme");
    const { container } = render(<EmptyState variant="illustrated" title="Nothing here" art={ART} />);
    expect(container.querySelector("img")?.getAttribute("src")).toBe("/empty-state/general/empty-light.svg");
    document.documentElement.setAttribute("data-theme", "dark");
    await waitFor(() =>
      expect(container.querySelector("img")?.getAttribute("src")).toBe("/empty-state/general/empty-dark.svg")
    );
    document.documentElement.setAttribute("data-theme", "light");
    await waitFor(() =>
      expect(container.querySelector("img")?.getAttribute("src")).toBe("/empty-state/general/empty-light.svg")
    );
  });

  it("keeps the initial read on hosts without a MutationObserver", () => {
    vi.stubGlobal("MutationObserver", undefined);
    try {
      document.documentElement.setAttribute("data-theme", "dark");
      const { container } = render(<EmptyState variant="illustrated" title="Nothing here" art={ART} />);
      expect(container.querySelector("img")?.getAttribute("src")).toBe("/empty-state/general/empty-dark.svg");
    } finally {
      vi.unstubAllGlobals();
    }
  });
});
