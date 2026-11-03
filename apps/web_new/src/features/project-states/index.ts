// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the project workflow-states feature. Other features and
// routes import only from here.

export {
  StatesTab,
  deleteBlockedReason,
  DELETE_DEFAULT_REASON,
  DELETE_LAST_IN_GROUP_REASON,
  SKELETON_ROWS,
} from "./components/StatesTab.js";
export { StateGroupCard } from "./components/StateGroupCard.js";
export type { StateGroupCardProps } from "./components/StateGroupCard.js";
export { StateRow } from "./components/StateRow.js";
export type { StateRowProps } from "./components/StateRow.js";
export { StateForm, STATE_FORM_PALETTE } from "./components/StateForm.js";
export type { StateFormProps, StateFormValue } from "./components/StateForm.js";
export { StateDeleteDialog } from "./components/StateDeleteDialog.js";
export type { StateDeleteDialogProps } from "./components/StateDeleteDialog.js";
export { MemberEditingToggle } from "./components/MemberEditingToggle.js";
export type { MemberEditingToggleProps } from "./components/MemberEditingToggle.js";
export { GroupGlyph } from "./components/GroupGlyph.js";
export type { GroupGlyphProps } from "./components/GroupGlyph.js";

export {
  canDragGroupRows,
  compareStateSequence,
  groupStates,
  isWorkflowGroup,
  rowShare,
  WORKFLOW_GROUPS,
  WORKFLOW_GROUP_LABELS,
  WORKFLOW_GROUP_SWATCHES,
  COLLAPSED_ON_LOAD,
} from "./groups.js";
export type { GroupedStates, WorkflowGroup } from "./groups.js";

export { neighborSequence, resolveStateMove, STATE_ORDER_STEP } from "./ordering.js";
export type { DropEdge, StateMove } from "./ordering.js";

export { canEditStates, canToggleMemberEditing } from "./rights.js";
export { capStateName, STATE_NAME_MAX_LENGTH, STATE_NAME_REQUIRED, validateStateName } from "./validation.js";
export * from "./notices.js";

export {
  appendState,
  applyDefaultFlag,
  applyStateMove,
  applyStatePatch,
  removeState,
  sequenceAfterGroup,
  snapshotsOf,
  stateKeys,
  statesQueryOptions,
  useCreateState,
  useDeleteState,
  useMarkStateDefault,
  useMoveState,
  useProjectStates,
  useSetMemberEditing,
  useUpdateState,
} from "./api.js";
