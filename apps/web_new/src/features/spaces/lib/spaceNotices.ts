// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Background-outcome notices (parity/specs/space.md SPC-017). One
// helper per outcome the board reports: copy confirmations, failed
// writes (comments, votes, reactions, edits, deletes, uploads) and the
// delete confirmation. The space layout mounts the kit toast host; the
// slices that own each write call these instead of wording their own,
// so the texts stay stable under the message keys. Sign-out needs no
// notice — its native form post navigates away — and a posted comment
// needs none either: the new row is its confirmation.
import { notifyError, notifySuccess } from "../../../shared/shell/toaster.js";

import { spaceMessage } from "./spaceMessages.js";

/** The issue link landed on the clipboard. */
export function notifySpaceCopySuccess(): void {
  notifySuccess(spaceMessage("notice.copySuccess.title"), spaceMessage("notice.copySuccess.body"));
}

/** The clipboard refused the issue link. */
export function notifySpaceCopyFailure(): void {
  notifyError(spaceMessage("notice.copyFailure.title"), spaceMessage("notice.copyFailure.body"));
}

/** A comment post failed; the thread and the draft are untouched. */
export function notifySpaceCommentFailure(): void {
  notifyError(spaceMessage("notice.commentFailure.title"), spaceMessage("notice.commentFailure.body"));
}

/** A vote write failed; the stored votes are shown instead. */
export function notifySpaceVoteFailure(): void {
  notifyError(spaceMessage("notice.voteFailure.title"), spaceMessage("notice.voteFailure.body"));
}

/** A reaction write failed; the stored reactions are shown instead. */
export function notifySpaceReactionFailure(): void {
  notifyError(spaceMessage("notice.reactionFailure.title"), spaceMessage("notice.reactionFailure.body"));
}

/** A comment edit failed; the stored body is shown instead. */
export function notifySpaceCommentEditFailure(): void {
  notifyError(spaceMessage("notice.commentEditFailure.title"), spaceMessage("notice.commentEditFailure.body"));
}

/** A comment delete landed. */
export function notifySpaceCommentDeleteSuccess(): void {
  notifySuccess(spaceMessage("notice.commentDeleteSuccess.title"));
}

/** A comment delete failed; the row stays. */
export function notifySpaceCommentDeleteFailure(): void {
  notifyError(spaceMessage("notice.commentDeleteFailure.title"), spaceMessage("notice.commentDeleteFailure.body"));
}

/** A comment attachment upload failed. */
export function notifySpaceAttachmentFailure(): void {
  notifyError(spaceMessage("notice.attachmentFailure.title"), spaceMessage("notice.attachmentFailure.body"));
}
