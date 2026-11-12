// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Action-row seam for the relations and links widgets (parity/specs/issues.md
// §15, ISS-173/180/185). The widget root owns the five-entry action row, while
// the dialogs live inside each widget's section. This context lets one
// "Add relation" menu or "Add link" button in the row open the matching
// dialog without either component reaching into the other's state.

import { Button, Menu } from "@pidash/kit";
import { Link2, Plus, Split } from "lucide-react";
import * as React from "react";

import { isEpicDialogEnabled } from "../edition/EpicDialog.js";

import { RELATION_KIND_SPECS, RELATION_KINDS, type RelationKind } from "./relationsModel.js";

export interface WidgetActionsValue {
  /** Relation kind chosen from the row menu; null closes the dialog. */
  relationKind: RelationKind | null;
  setRelationKind: (kind: RelationKind | null) => void;
  /** Whether the add/edit link dialog is open. */
  linkOpen: boolean;
  setLinkOpen: (open: boolean) => void;
  /** Whether the blank epic dialog is open (cloud builds only). */
  epicCreateOpen: boolean;
  setEpicCreateOpen: (open: boolean) => void;
}

const WidgetActionsContext = React.createContext<WidgetActionsValue | null>(null);

export function WidgetActionsProvider({ children }: { children: React.ReactNode }): React.ReactElement {
  const [relationKind, setRelationKind] = React.useState<RelationKind | null>(null);
  const [linkOpen, setLinkOpen] = React.useState(false);
  const [epicCreateOpen, setEpicCreateOpen] = React.useState(false);
  const value = React.useMemo(
    () => ({ relationKind, setRelationKind, linkOpen, setLinkOpen, epicCreateOpen, setEpicCreateOpen }),
    [relationKind, linkOpen, epicCreateOpen]
  );
  return <WidgetActionsContext.Provider value={value}>{children}</WidgetActionsContext.Provider>;
}

/** The shared row state; the widgets always render under the provider. */
export function useWidgetActions(): WidgetActionsValue {
  const value = React.useContext(WidgetActionsContext);
  if (value === null) throw new Error("Widget actions must be provided by WidgetActionsProvider.");
  return value;
}

/** Action-row control for relations: the four kinds, in menu order. */
export function RelationActionMenu({ canEdit }: { canEdit: boolean }): React.ReactElement {
  const actions = useWidgetActions();
  return (
    <Menu
      trigger={
        <Button
          variant="secondary"
          size="medium"
          disabled={!canEdit || actions === null}
          data-widget-action="Add relation"
        >
          <span className="flex items-center gap-(--space-1)">
            <Split size={14} strokeWidth={1.5} aria-hidden="true" />
            <span>Add relation</span>
          </span>
        </Button>
      }
      items={RELATION_KINDS.map((kind) => {
        const spec = RELATION_KIND_SPECS[kind];
        const Icon = spec.icon;
        return {
          label: spec.label,
          icon: <Icon size={14} strokeWidth={1.5} aria-hidden="true" />,
          onSelect: () => actions?.setRelationKind(kind),
        };
      })}
    />
  );
}

/** Action-row control for links; the widget owns the dialog. */
export function LinkActionButton({ canEdit }: { canEdit: boolean }): React.ReactElement {
  const actions = useWidgetActions();
  return (
    <Button
      variant="secondary"
      size="medium"
      disabled={!canEdit || actions === null}
      data-widget-action="Add link"
      onClick={() => actions?.setLinkOpen(true)}
    >
      <span className="flex items-center gap-(--space-1)">
        <Link2 size={14} strokeWidth={1.5} aria-hidden="true" />
        <span>Add link</span>
      </span>
    </Button>
  );
}

/**
 * Action-row control for epic creation (ISS-232). The community build has
 * no epics, so this draws nothing at all; a cloud build offers the blank
 * epic form beside the relation and link entries, and a viewer without
 * edit standing never sees it.
 */
export function EpicCreateButton({ canEdit }: { canEdit: boolean }): React.ReactElement | null {
  const actions = useWidgetActions();
  if (!isEpicDialogEnabled()) return null;
  return (
    <Button
      variant="secondary"
      size="medium"
      disabled={!canEdit}
      data-widget-action="Create epic"
      onClick={() => actions.setEpicCreateOpen(true)}
    >
      <span className="flex items-center gap-(--space-1)">
        <Plus size={14} strokeWidth={1.5} aria-hidden="true" />
        <span>Create epic</span>
      </span>
    </Button>
  );
}
