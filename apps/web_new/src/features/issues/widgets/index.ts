// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Relations and links widgets (parity/specs/issues.md §15, ISS-179–186).

export { IssueWidgets } from "./IssueWidgets.js";
export type { IssueWidgetsProps } from "./IssueWidgets.js";
export { LinksWidget, addedAgo, attribution, faviconUrl, linkTitle } from "./LinksWidget.js";
export type { LinksWidgetProps } from "./LinksWidget.js";
export { RelationsWidget } from "./RelationsWidget.js";
export type { RelationsWidgetProps } from "./RelationsWidget.js";
export {
  INVERSE_RELATION,
  RELATION_KINDS,
  RELATION_KIND_SPECS,
  isRelationKind,
  relationCount,
  relationRows,
  visibleRelationGroups,
} from "./relationsModel.js";
export type { RelationKind, RelationKindSpec } from "./relationsModel.js";
export { normalizeLinkUrl } from "./api.js";
export { loadWidgetOpen, storeWidgetOpen, widgetOpenKey, WidgetAddButton, WidgetSection } from "./WidgetSection.js";
export type { WidgetSectionProps } from "./WidgetSection.js";
export { AddRelationDialog, RELATION_EMPTY_SELECTION } from "./AddRelationDialog.js";
export type { AddRelationDialogProps } from "./AddRelationDialog.js";
export { LinkDialog, LINK_URL_REQUIRED } from "./LinkDialog.js";
export type { LinkDialogProps, LinkDraft } from "./LinkDialog.js";
export { RelationRow, copyWorkItemUrl } from "./RelationRow.js";
export type { RelationRowProps } from "./RelationRow.js";
