// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Test stand-in for the lazy Tiptap chat input (shared/editor/ChatInput).
// Composer and home-card suites test their own send/lock logic, not the
// editor, so they swap the contenteditable for a native textarea that
// honours the same contract: a string draft, the same Enter / Shift+Enter
// / chord decision (chatInputKeyAction), the same label and lock. The
// editor itself is covered against real Tiptap in ChatInput.test.tsx.
//
//   vi.mock("<path>/shared/editor/ChatInput.js", () => import("<path>/testSupport/chatInputStub.js"));

import * as React from "react";

import type { ChatInputProps } from "../shared/editor/ChatInput.js";
import { chatInputKeyAction } from "../shared/editor/chatInputText.js";

export type { ChatInputProps };

export function ChatInput({
  value,
  onChange,
  onSubmit,
  label,
  placeholder,
  disabled = false,
  maxHeight,
  className,
}: ChatInputProps): React.ReactElement {
  return (
    <textarea
      aria-label={label}
      placeholder={placeholder}
      value={value}
      disabled={disabled}
      data-max-height={maxHeight}
      className={className}
      onChange={(event) => onChange(event.target.value)}
      onKeyDown={(event) => {
        const action = chatInputKeyAction(event);
        if (action === "pass" || action === "newline") return;
        event.preventDefault();
        if (action === "submit") onSubmit();
      }}
    />
  );
}
