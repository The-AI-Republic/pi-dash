// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { RichTextView } from "./RichTextView.js";
import { resetEntityPeek, subscribeEntityPeek } from "../shell/entityPeek.js";

afterEach(() => {
  resetEntityPeek();
});

describe("RichTextView", () => {
  it("renders stored markup under its label", () => {
    render(<RichTextView html="<p>Hello <strong>there</strong></p>" label="Description" />);
    const region = screen.getByRole("region", { name: "Description" });
    expect(region).toContainHTML("<strong>there</strong>");
    expect(region).toHaveTextContent("Hello there");
  });

  it("strips unsafe markup before rendering", () => {
    render(<RichTextView html='<p>ok</p><script>alert(1)</script><img src="x" onerror="alert(2)"> ' label="Note" />);
    const region = screen.getByRole("region", { name: "Note" });
    expect(region.querySelector("script")).toBeNull();
    expect(region.innerHTML).not.toContain("onerror");
    expect(region).toHaveTextContent("ok");
  });

  it("appends a caller class to the content frame", () => {
    render(<RichTextView html="<p>body</p>" label="Body" className="mt-4" />);
    const region = screen.getByRole("region", { name: "Body" });
    expect(region).toHaveClass("rich-text-content", "mt-4");
  });

  it("re-sanitizes when the stored HTML changes", () => {
    const { rerender } = render(<RichTextView html="<p>first</p>" label="Body" />);
    rerender(<RichTextView html="<p>second</p>" label="Body" />);
    expect(screen.getByRole("region", { name: "Body" })).toHaveTextContent("second");
  });

  it("shows a stored mention as the person's name (ISS-127)", () => {
    render(
      <RichTextView
        html={
          '<p>hi <mention-component id="u-1" entity_identifier="u-1" entity_name="user_mention" data-name="Ada"></mention-component> there</p>'
        }
        label="Description"
      />
    );
    const region = screen.getByRole("region", { name: "Description" });
    expect(region).toHaveTextContent("hi @Ada there");
    expect(region.querySelector('[data-mention-id="u-1"]')).not.toBeNull();
  });

  it("links a stored mention to the member profile when the workspace is known (CMT-020)", () => {
    render(
      <RichTextView
        html={
          '<p>hi <mention-component id="u-1" entity_identifier="u-1" entity_name="user_mention" data-name="Ada"></mention-component> there</p>'
        }
        label="Description"
        workspaceSlug="acme"
      />
    );
    const link = screen.getByRole("link", { name: "@Ada" });
    expect(link.getAttribute("href")).toBe("/acme/profile/u-1");
    expect(link).toHaveClass("mention-chip");
    expect(link.getAttribute("data-mention-id")).toBe("u-1");
  });

  it("shows a stored image component as an image (ISS-127)", () => {
    render(
      <RichTextView html='<image-component src="https://cdn.test/a.png" alt="shot"></image-component>' label="Body" />
    );
    const image = screen.getByRole("region", { name: "Body" }).querySelector("img");
    expect(image?.getAttribute("src")).toBe("https://cdn.test/a.png");
    expect(image?.getAttribute("alt")).toBe("shot");
  });

  it("resolves a stored asset id through the caller's scope (ISS-127)", () => {
    render(
      <RichTextView
        html='<image-component src="35e8b958-6ee5-43ce-ae56-fb0e776f421e" status="uploaded"></image-component>'
        label="Body"
        resolveAsset={(src) => `/api/assets/v2/workspaces/acme/projects/p-1/${src}/`}
      />
    );
    const image = screen.getByRole("region", { name: "Body" }).querySelector("img");
    expect(image?.getAttribute("src")).toBe(
      "/api/assets/v2/workspaces/acme/projects/p-1/35e8b958-6ee5-43ce-ae56-fb0e776f421e/"
    );
  });

  it("renders stored callouts, embeds and entity previews (PAGE-037/040/043)", () => {
    render(
      <RichTextView
        html={
          '<callout-component data-type="callout"><p>Heads up</p></callout-component>' +
          '<issue-embed-component id="emb-1" entity_identifier="ISS-1" entity_name="Embedded"></issue-embed-component>' +
          '<entity-reference-component entity_name="issue" entity_identifier="ISS-2" data-label="Fix login"></entity-reference-component>'
        }
        label="Body"
      />
    );
    const region = screen.getByRole("region", { name: "Body" });
    expect(region.querySelector('aside[data-type="callout"]')).toHaveTextContent("Heads up");
    expect(region.querySelector(".page-embed-upgrade")).toHaveTextContent("Pro feature");
    expect(region.querySelector(".page-embed-upgrade a")?.getAttribute("href")).toContain("pricing");
    expect(region.querySelector('[data-entity-preview][data-entity-identifier="ISS-2"]')).toHaveTextContent(
      "Fix login"
    );
  });

  it("activates an entity preview by click and keyboard for the shell peek (PAGE-043)", () => {
    const seen = vi.fn();
    const unsubscribe = subscribeEntityPeek(seen);
    render(
      <RichTextView
        html={
          '<p><entity-reference-component entity_name="issue" entity_identifier="ISS-2" project_identifier="P" workspace_identifier="acme" project_id="p-1" data-label="Fix login"></entity-reference-component></p>'
        }
        label="Body"
      />
    );
    const preview = screen.getByRole("button", { name: "Fix login" });
    fireEvent.click(preview);
    expect(seen).toHaveBeenLastCalledWith({
      entityName: "issue",
      entityIdentifier: "ISS-2",
      projectIdentifier: "P",
      workspaceIdentifier: "acme",
      projectId: "p-1",
      label: "Fix login",
    });
    fireEvent.keyDown(preview, { key: "Enter" });
    expect(seen).toHaveBeenCalledTimes(2);
    fireEvent.keyDown(preview, { key: "Space" });
    expect(seen).toHaveBeenCalledTimes(2);
    unsubscribe();
  });
});
