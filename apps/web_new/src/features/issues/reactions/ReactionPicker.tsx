// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Standalone picker bound to one reaction collection (parity/specs/issues.md
// §16, ISS-203/204). The row's add button uses the same picker inline; the
// comment header mounts this one so the picker sits beside the card menu.

import * as React from "react";

import { EmojiPicker } from "./EmojiPicker.js";

export interface ReactionPickerProps {
  onPick: (emoji: string) => void;
  disabled?: boolean;
}

export function ReactionPicker({ onPick, disabled = false }: ReactionPickerProps): React.ReactElement {
  return <EmojiPicker onPick={onPick} disabled={disabled} />;
}
