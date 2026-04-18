/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { Control } from "react-hook-form";
// pi dash imports
import type { EditorRefApi } from "@pi-dash/editor";
// types
import type { TBulkIssueProperties, TIssue } from "@pi-dash/types";

export type TIssueFields = TIssue & TBulkIssueProperties;

export type TIssueTypeDropdownVariant = "xs" | "sm";

export type TIssueTypeSelectProps<T extends Partial<TIssueFields>> = {
  control: Control<T>;
  projectId: string | null;
  editorRef?: React.MutableRefObject<EditorRefApi | null>;
  disabled?: boolean;
  variant?: TIssueTypeDropdownVariant;
  placeholder?: string;
  isRequired?: boolean;
  renderChevron?: boolean;
  dropDownContainerClassName?: string;
  showMandatoryFieldInfo?: boolean; // Show info about mandatory fields
  handleFormChange?: () => void;
};

// eslint-disable-next-line @typescript-eslint/no-unused-vars
export function IssueTypeSelect<T extends Partial<TIssueFields>>(props: TIssueTypeSelectProps<T>) {
  return <></>;
}
