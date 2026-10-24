// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { ToastHost } from "@pidash/kit";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { getToastManager } from "../../../shared/shell/toaster.js";

import {
  notifySpaceAttachmentFailure,
  notifySpaceCommentDeleteFailure,
  notifySpaceCommentDeleteSuccess,
  notifySpaceCommentEditFailure,
  notifySpaceCommentFailure,
  notifySpaceCopyFailure,
  notifySpaceCopySuccess,
  notifySpaceReactionFailure,
  notifySpaceVoteFailure,
} from "./spaceNotices.js";

function renderHost(): void {
  render(<ToastHost manager={getToastManager()} />);
}

describe("space notices", () => {
  it("confirms a copied issue link", async () => {
    renderHost();
    notifySpaceCopySuccess();
    expect(await screen.findByText("Board link copied")).toBeInTheDocument();
    expect(await screen.findByText("The link is on your clipboard, ready to share.")).toBeInTheDocument();
  });

  it("reports a refused clipboard write", async () => {
    renderHost();
    notifySpaceCopyFailure();
    expect(await screen.findByText("Couldn't copy the link")).toBeInTheDocument();
    expect(await screen.findByText("The clipboard refused the write — copy the address manually.")).toBeInTheDocument();
  });

  it("reports a failed comment post", async () => {
    renderHost();
    notifySpaceCommentFailure();
    expect(await screen.findByText("Couldn't post the comment")).toBeInTheDocument();
    expect(await screen.findByText("Your comment was not posted. Give it another try.")).toBeInTheDocument();
  });

  it("reports failed engagement writes", async () => {
    renderHost();
    notifySpaceVoteFailure();
    notifySpaceReactionFailure();
    expect(await screen.findByText("Your vote could not be saved. Please try again.")).toBeInTheDocument();
    expect(await screen.findByText("Your reaction could not be saved. Please try again.")).toBeInTheDocument();
  });

  it("reports failed comment edits and deletes", async () => {
    renderHost();
    notifySpaceCommentEditFailure();
    notifySpaceCommentDeleteFailure();
    expect(await screen.findByText("Your edit could not be saved. Please try again.")).toBeInTheDocument();
    expect(await screen.findByText("The comment could not be deleted. Please try again.")).toBeInTheDocument();
  });

  it("confirms a deleted comment without a body line", async () => {
    renderHost();
    notifySpaceCommentDeleteSuccess();
    expect(await screen.findByText("Comment deleted!")).toBeInTheDocument();
  });

  it("reports a failed attachment upload", async () => {
    renderHost();
    notifySpaceAttachmentFailure();
    expect(await screen.findByText("Couldn't attach the file")).toBeInTheDocument();
    expect(await screen.findByText("The upload failed. Please try again.")).toBeInTheDocument();
  });
});
