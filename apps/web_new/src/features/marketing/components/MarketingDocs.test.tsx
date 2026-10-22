// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Docs page (CLOUD-006). Counts, order and destinations are what the
// oracle pins; copy is free.
import { render } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { CLI_MSI_URL, DOCS_CONCEPTS, DOCS_STEPS, REPOSITORY_URL } from "../content.js";
import { MarketingDocs } from "./MarketingDocs.js";

function sections(container: HTMLElement): HTMLElement[] {
  return [...container.querySelectorAll("section")];
}

describe("MarketingDocs", () => {
  it("owns the page's only top-level heading and links sign-in plus the repository", () => {
    const { container } = render(<MarketingDocs />);
    expect(container.querySelectorAll("h1")).toHaveLength(1);
    const heroLinks = [...sections(container)[0]!.querySelectorAll("a")].map((anchor) => anchor.getAttribute("href"));
    expect(heroLinks).toEqual(["/login", REPOSITORY_URL]);
    expect(container.querySelector(`a[href="${REPOSITORY_URL}"]`)?.getAttribute("target")).toBe("_blank");
  });

  it("explains the three-part model as three articles", () => {
    const { container } = render(<MarketingDocs />);
    const concepts = [...container.querySelectorAll("article")];
    expect(concepts).toHaveLength(3);
    expect(concepts.map((article) => article.querySelector("h3")?.textContent)).toEqual(
      DOCS_CONCEPTS.map((concept) => concept.title)
    );
  });

  it("walks four quick-start steps in order", () => {
    const { container } = render(<MarketingDocs />);
    const steps = [...container.querySelectorAll("ol > li")];
    expect(steps).toHaveLength(4);
    expect(steps.map((step) => step.querySelector("h3")?.textContent)).toEqual(DOCS_STEPS.map((step) => step.title));
  });

  it("shows five command blocks with the release-download installers", () => {
    const { container } = render(<MarketingDocs />);
    const blocks = [...container.querySelectorAll("pre")];
    expect(blocks).toHaveLength(5);
    expect(blocks[0]?.textContent).toContain("releases/latest/download/install.sh");
    expect(blocks[1]?.textContent).toContain("releases/latest/download/install.ps1");
    expect(blocks[2]?.textContent).toContain("pidash auth login");
    expect(blocks[3]?.textContent).toContain("pidash runner add");
    expect(blocks[4]?.textContent).toContain("npx @airepublic/pidash-skill-installer");
  });

  it("links the MSI alternative and notes the subsystem boundary", () => {
    const { container } = render(<MarketingDocs />);
    expect(container.querySelector('a[href$=".msi"]')?.getAttribute("href")).toBe(CLI_MSI_URL);
    expect(container.textContent).toContain("Managed Windows deployments");
    expect(container.textContent).toContain("only drives agents inside that subsystem");
  });

  it("closes with the four-phase loop and three trust notes", () => {
    const { container } = render(<MarketingDocs />);
    const all = sections(container);
    expect(all).toHaveLength(6);
    expect(all[4]!.querySelectorAll("ul > li")).toHaveLength(4);
    expect(all[5]!.querySelectorAll(":scope > div > div")).toHaveLength(3);
  });
});
