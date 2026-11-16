// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Temporary filter row for static defaults (parity/specs/views.md §11,
// VIEW-038). Defaults carry no record, so there is nothing to save the
// row into: the builder runs over a temporary instance (workspace field
// configs, no project features) and reports its live expression for the
// body read. Edits die with the visit — deliberately, so one default's
// filters never leak into another's.

import {
  loadRichFilterBuilder,
  type IssuesEntity,
  type RichExpression,
  type RichFilterBuilderModule,
  type RichFilterBuilderParts,
} from "../../issues/index.js";
import * as React from "react";

export interface WorkspaceDefaultFilterSlots {
  control: React.ReactNode;
  row: React.ReactNode;
}

export interface WorkspaceDefaultFiltersProps {
  workspaceSlug: string;
  /** Live expression for the body read; reported on every row change. */
  onExpressionChange: (expression: RichExpression) => void;
  children: (slots: WorkspaceDefaultFilterSlots) => React.ReactNode;
}

export function WorkspaceDefaultFilters({
  workspaceSlug,
  onExpressionChange,
  children,
}: WorkspaceDefaultFiltersProps): React.ReactElement {
  const [module, setModule] = React.useState<RichFilterBuilderModule | null>(null);
  const [failed, setFailed] = React.useState(false);
  React.useEffect(() => {
    let live = true;
    void loadRichFilterBuilder().then(
      (loaded) => {
        if (live) setModule(loaded);
      },
      () => {
        if (live) setFailed(true);
      }
    );
    return () => {
      live = false;
    };
  }, []);
  if (failed) {
    return <>{children({ control: null, row: <p role="alert">Filters could not be loaded.</p> })}</>;
  }
  if (module === null) {
    return <>{children({ control: null, row: null })}</>;
  }
  return (
    <TemporaryFilters module={module} workspaceSlug={workspaceSlug} onExpressionChange={onExpressionChange}>
      {children}
    </TemporaryFilters>
  );
}

interface TemporaryFiltersProps {
  module: RichFilterBuilderModule;
  workspaceSlug: string;
  onExpressionChange: (expression: RichExpression) => void;
  children: (slots: WorkspaceDefaultFilterSlots) => React.ReactNode;
}

function TemporaryFilters({
  module,
  workspaceSlug,
  onExpressionChange,
  children,
}: TemporaryFiltersProps): React.ReactElement {
  const entity = React.useMemo<IssuesEntity>(() => ({ scope: "workspace" }), []);
  const parts: RichFilterBuilderParts = module.useRichFilterBuilder({
    workspaceSlug,
    projectId: "",
    entity,
    hideCycle: true,
    hideModule: true,
    temporary: true,
  });
  // The builder only swaps the conditions identity on a committed change,
  // so this reports row edits without looping on parent re-renders.
  React.useEffect(() => {
    onExpressionChange(module.serializeRichExpression(parts.conditions));
  }, [module, parts.conditions, onExpressionChange]);
  return <>{children({ control: parts.control, row: parts.row })}</>;
}
