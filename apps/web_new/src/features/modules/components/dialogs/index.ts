// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module dialog exports (MOD-003/009/022–025). Surfaces outside the
// modules feature open the dialogs only through these openers.

export { ModuleCreateAction, MODULE_ADD_LABEL, MODULE_ADD_LABEL_COMPACT } from "./ModuleCreateAction.js";
export type { ModuleCreateActionProps } from "./ModuleCreateAction.js";
export { ModuleDeleteDialog } from "./ModuleDeleteDialog.js";
export {
  ModuleDetailActions,
  MODULE_MENU_DELETE,
  MODULE_MENU_EDIT,
  type ModuleDetailActionsProps,
} from "./ModuleDetailActions.js";
export type { ModuleDeleteDialogProps } from "./ModuleDeleteDialog.js";
export {
  ModuleDialogProvider,
  useModuleDialogs,
  type ModuleDeleteOptions,
  type ModuleDeleteTarget,
  type ModuleDialogHandle,
  type ModuleDialogProviderProps,
} from "./ModuleDialogProvider.js";
export { ModuleForm, type ModuleFormProject, type ModuleFormProps } from "./ModuleForm.js";
export { ModuleSaveDialog, type ModuleSaveDialogProps } from "./ModuleSaveDialog.js";
