// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Copy entry with a blank-reading handle (bug NEWFRONT-360): the mounted
// editor can serialize blank while the visible body holds content, and the
// copy entry must carry the visible body rather than the blank read.

import { render } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { getPageBodyActions, resetPageBodyActions } from "../lib/pageBodyActions.js";
import { resetPageBodyDrafts } from "../lib/pageBodyDraft.js";
import { PageEditorSurface } from "./PageEditorSurface.js";

const toaster = vi.hoisted(() => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));
vi.mock("../../../shared/shell/toaster.js", () => toaster);

const appClient = vi.hoisted(() => ({ getAppClient: vi.fn(() => ({ kind: "app-client" })) }));
vi.mock("../../../core/api/client.js", () => ({ getAppClient: appClient.getAppClient }));

// A mounted editor whose handle serializes blank and never reports an
// edit: the stuck-mount state NEWFRONT-360 pins, with the visible body
// still holding the stored words.
vi.mock("./PageBodyEditor.js", () => ({
  PageBodyEditor: ({ onReady }: { onReady: (handle: unknown) => void }): React.ReactElement | null => {
    React.useEffect(() => {
      // The bar is off, so the formatting snapshot is never read.
      onReady({ html: () => "", readState: () => ({}) });
    }, [onReady]);
    return null;
  },
}));

vi.mock("./PageBodyInteractionLayer.js", () => ({
  PageBodyInteractionLayer: (): React.ReactElement | null => null,
}));

afterEach(() => {
  resetPageBodyActions();
  resetPageBodyDrafts();
  toaster.notifyError.mockReset();
  toaster.notifySuccess.mockReset();
  vi.unstubAllGlobals();
});

describe("PageEditorSurface copy with a blank-reading handle", () => {
  it("copies the visible body instead of the blank live read", async () => {
    const writeText = vi.fn(async () => undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    render(
      <PageEditorSurface
        pageId="page-1"
        workspaceSlug="acme"
        projectId="project-1"
        name="Launch notes"
        descriptionHtml="<p>Stored words</p>"
        logoProps={{}}
        onRename={vi.fn()}
        onMarkPick={vi.fn()}
        showBar={false}
      />
    );
    await vi.waitFor(() => expect(getPageBodyActions()).not.toBeNull());
    await getPageBodyActions()?.copyMarkdown();
    expect(writeText).toHaveBeenCalledWith("Stored words\n");
  });
});
