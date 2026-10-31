// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the prompts feature. Other features import only from here.

export { promptReceiptKeys, promptSectionKeys } from "./api/keys.js";
export type { PromptSectionScope } from "./api/keys.js";
export {
  MISSING_INSTALL_TARGET,
  MISSING_ISSUE_TARGET,
  PERSONAL_SAVED_TITLE,
  PREVIEW_FAILED_FALLBACK,
  previewFailureMessage,
  REVERT_FAILED_MESSAGE,
  SECTION_REVERTED_TITLE,
  saveFailureDetail,
  saveFailureMessage,
  usePreviewPrompt,
  useRevertPromptSection,
  useUpsertPromptSection,
  WORKSPACE_SAVED_TITLE,
} from "./api/mutations.js";
export type { PromptPreviewRequest, PromptRevertInput, PromptUpsertInput } from "./api/mutations.js";
export {
  compiledPromptQueryOptions,
  promptSectionsQueryOptions,
  usePromptReceipts,
  usePromptSectionLists,
} from "./api/queries.js";
export type { PromptReceiptEntry, PromptReceipts, PromptSectionLists } from "./api/queries.js";
export { PreviewPanel } from "./components/PreviewPanel.js";
export type { PreviewDraft, PreviewPanelProps } from "./components/PreviewPanel.js";
export { PromptsTabs, PROMPT_TABS } from "./components/PromptsTabs.js";
export type { PromptTab, PromptsTabsProps } from "./components/PromptsTabs.js";
export { PromptsView } from "./components/PromptsView.js";
export type { PromptsViewProps } from "./components/PromptsView.js";
export { RevertDialog, revertDialogBody } from "./components/RevertDialog.js";
export type { RevertDialogProps } from "./components/RevertDialog.js";
export { AUTOMATIC_RUNS_LABEL, ReceiptCard } from "./components/ReceiptCard.js";
export type { ReceiptCardProps } from "./components/ReceiptCard.js";
export { ReceiptView, RECEIPTS_FAILED_TEXT, RECEIPTS_LOADING_TEXT } from "./components/ReceiptView.js";
export type { ReceiptViewProps } from "./components/ReceiptView.js";
export { SectionCard, STALE_OVERRIDE_WARNING } from "./components/SectionCard.js";
export type { SectionCardProps } from "./components/SectionCard.js";
export { SectionEditor } from "./components/SectionEditor.js";
export type { SectionEditorProps } from "./components/SectionEditor.js";
export {
  CUSTOMIZE_PERSONAL_LABEL,
  CUSTOMIZE_WORKSPACE_LABEL,
  EDIT_PERSONAL_LABEL,
  EDIT_WORKSPACE_LABEL,
  editLabelsFor,
  editorScopeLabel,
  editorSeedFor,
  hasPersonalOverride,
  hasWorkspaceOverride,
  mergePromptSections,
  PROMPT_KIND_LABELS,
  PROMPT_KINDS,
  revertArmedFor,
  sourceBadgeFor,
  tierMarkFor,
} from "./lib/sections.js";
export type {
  MergedPromptSection,
  PromptEditorScope,
  PromptKind,
  PromptKindSections,
  PromptSourceBadge,
  PromptTierMark,
  PromptViewerGates,
} from "./lib/sections.js";
export {
  automaticTemplate,
  draftPreviewKinds,
  initialDraftKind,
  previewTargetLabel,
  previewTargetMode,
  previewTargetPayload,
  previewTargetPlaceholder,
  receiptAnchorId,
  receiptSlots,
  sectionCountLabel,
} from "./lib/receipt.js";
export type { PreviewTargetMode, ReceiptSlot } from "./lib/receipt.js";
