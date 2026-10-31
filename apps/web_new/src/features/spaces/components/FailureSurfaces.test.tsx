// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { FORUM_URL } from "../../../shared/shell/externalLinks.js";
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { LANGUAGE_STORAGE_KEY, registerLocale, resetLocalesForTests } from "../../../core/i18n/locales.js";
import { registerSpaceMessages, resetSpaceMessagesForTests } from "../lib/spaceMessages.js";
import {
  SPACE_ANCHOR_NOT_FOUND_ART,
  SPACE_BOARD_ERROR_ART,
  SpaceAnchorNotFound,
  SpaceBoardError,
} from "./FailureSurfaces.js";
import { SPACE_HEAD_DESCRIPTION_FALLBACK, SPACE_HEAD_TITLE_FALLBACK } from "../lib/boardHead.js";
import { reloadSpaceCrash, SpaceCrashScreen, SPACE_CRASH_HEADING, SPACE_SUPPORT_HREF } from "./CrashScreen.js";
import {
  reloadSpacePage,
  SpaceInstanceFailureScreen,
  SPACE_INSTANCE_FAILURE_ART,
  SPACE_INSTANCE_FAILURE_HEADING,
} from "./InstanceFailureScreen.js";

afterEach(() => {
  document.title = "";
  for (const tag of [...document.head.querySelectorAll("meta")]) tag.remove();
  globalThis.localStorage?.clear();
  resetSpaceMessagesForTests();
  resetLocalesForTests();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function expectGenericFallbackTags(): void {
  expect(document.head.querySelector('meta[name="robots"]')).toHaveAttribute("content", "noindex, nofollow");
  expect(document.head.querySelector('meta[name="description"]')).toHaveAttribute(
    "content",
    SPACE_HEAD_DESCRIPTION_FALLBACK
  );
  expect(document.head.querySelector('meta[property="og:title"]')).toHaveAttribute(
    "content",
    SPACE_HEAD_TITLE_FALLBACK
  );
  expect(document.head.querySelector('meta[property="og:description"]')).toHaveAttribute(
    "content",
    SPACE_HEAD_DESCRIPTION_FALLBACK
  );
  expect(document.head.querySelector('meta[name="twitter:title"]')).toHaveAttribute(
    "content",
    SPACE_HEAD_TITLE_FALLBACK
  );
  expect(document.head.querySelector('meta[name="twitter:description"]')).toHaveAttribute(
    "content",
    SPACE_HEAD_DESCRIPTION_FALLBACK
  );
  expect(document.head.querySelector('meta[property="og:image"]')).toBeNull();
  expect(document.head.querySelector('meta[name="twitter:image"]')).toBeNull();
}

describe("SpaceAnchorNotFound", () => {
  it("renders the anchor illustration and no board chrome", () => {
    render(<SpaceAnchorNotFound />);
    expect(screen.getByTestId("space-anchor-not-found")).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 3, name: "We couldn't find this board" })).toBeInTheDocument();
    expect(screen.getByRole("img")).toHaveAttribute("src", SPACE_ANCHOR_NOT_FOUND_ART);
    expect(document.querySelector("header")).toBeNull();
    expect(document.title).toContain("Board not found");
  });

  it("keeps its own title and noindex while carrying the generic description and social tags", () => {
    render(<SpaceAnchorNotFound />);
    expect(document.title).toContain("Board not found");
    expectGenericFallbackTags();
  });
});

describe("SpaceBoardError", () => {
  it("renders the board-error illustration with a primary heading", () => {
    render(<SpaceBoardError />);
    expect(screen.getByTestId("space-board-error")).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 1, name: "We couldn't load this board" })).toBeInTheDocument();
    expect(screen.getByRole("img")).toHaveAttribute("src", SPACE_BOARD_ERROR_ART);
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("keeps its own title and noindex while carrying the generic description and social tags", () => {
    render(<SpaceBoardError />);
    expect(document.title).toContain("Board error");
    expectGenericFallbackTags();
  });
});

describe("failure surfaces through the message catalog", () => {
  it("renders the registered pack entry with English fallback per key", () => {
    registerLocale({ tag: "fr", label: "Francais" });
    globalThis.localStorage.setItem(LANGUAGE_STORAGE_KEY, "fr");
    registerSpaceMessages("fr", { "failure.board.heading": "Echec du chargement" });
    render(<SpaceBoardError />);
    expect(screen.getByRole("heading", { level: 1, name: "Echec du chargement" })).toBeInTheDocument();
    expect(screen.getByRole("img")).toHaveAttribute("alt", "An unavailable server");
  });
});

describe("SpaceInstanceFailureScreen", () => {
  it("shows the wordmark, illustration, explanation and retry reload", () => {
    const reload = vi.fn();
    vi.stubGlobal("window", { location: { reload } });
    render(<SpaceInstanceFailureScreen />);
    expect(screen.getByTestId("space-instance-failure")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: SPACE_INSTANCE_FAILURE_HEADING })).toBeInTheDocument();
    expect(document.querySelector(`img[src="${SPACE_INSTANCE_FAILURE_ART}"]`)).not.toBeNull();
    expect(screen.getByRole("link")).toHaveAttribute("href", "/");
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(reload).toHaveBeenCalledTimes(1);
    reloadSpacePage();
    expect(reload).toHaveBeenCalledTimes(2);
  });
});

describe("SpaceCrashScreen", () => {
  it("offers refresh plus support and forum contacts", () => {
    const reload = vi.fn();
    vi.stubGlobal("window", { location: { reload } });
    render(<SpaceCrashScreen />);
    expect(screen.getByTestId("space-crash")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: SPACE_CRASH_HEADING })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Email support" })).toHaveAttribute("href", SPACE_SUPPORT_HREF);
    expect(screen.getByRole("link", { name: "Community forum" })).toHaveAttribute("href", FORUM_URL);
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(reload).toHaveBeenCalledTimes(1);
    reloadSpaceCrash();
    expect(reload).toHaveBeenCalledTimes(2);
  });
});
