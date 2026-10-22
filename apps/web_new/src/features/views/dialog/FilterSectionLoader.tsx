// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Lazy filter section for the view dialog (parity/specs/views.md §5,
// VIEW-013). The temporary builder and its registry arrive through the
// issues lazy loader, never a static barrel re-export (which would pin
// the filter subtree into the initial bundle): while its chunk loads
// the section shows a loading note, and a failed load surfaces an
// error note instead of the builder. The host owns the registry the
// temporary instance reports into and hands it to the dialog through
// the ref, so submitting reads whatever the builder holds. An edit
// dialog seeds the instance with the record's stored expression.

import * as React from "react";

import {
  loadRichFilterBuilder,
  type FieldConfigRegistry,
  type IssuesEntity,
  type RichFilterBuilderModule,
} from "../../issues/index.js";

export interface FilterSectionLoaderProps {
  workspaceSlug: string;
  projectId: string;
  entity: IssuesEntity;
  /** Cycle/module properties hide while their project features are off. */
  hideCycle: boolean;
  hideModule: boolean;
  /** Receives the temporary registry once the builder chunk has loaded. */
  registryRef: { current: FieldConfigRegistry | null };
  /**
   * Stored expression an edit dialog reopens with (VIEW-035). Parsed
   * inside the loaded chunk so the parser never joins the eager graph;
   * absent on create, where the builder starts empty.
   */
  seedExpression?: Record<string, unknown> | undefined;
}

interface FilterSectionHostProps extends FilterSectionLoaderProps {
  module: RichFilterBuilderModule;
}

/** The builder itself, rendered once its chunk has loaded. */
function FilterSectionHost({
  module,
  workspaceSlug,
  projectId,
  entity,
  hideCycle,
  hideModule,
  registryRef,
  seedExpression,
}: FilterSectionHostProps): React.ReactElement {
  const registry = React.useMemo(() => new module.FieldConfigRegistry(), [module]);
  React.useEffect(() => {
    registryRef.current = registry;
    return () => {
      registryRef.current = null;
    };
  }, [registry, registryRef]);
  const seed = seedExpression === undefined ? undefined : module.parseRichExpression(seedExpression);
  const { control, row } = module.useRichFilterBuilder({
    workspaceSlug,
    projectId,
    entity,
    hideCycle,
    hideModule,
    temporary: true,
    temporaryRegistry: registry,
    ...(seed === undefined ? {} : { temporarySeed: seed }),
  });
  return (
    <>
      {control}
      {row}
    </>
  );
}

/** Filter section with its chunk loading behind a loading note. */
export function FilterSectionLoader(props: FilterSectionLoaderProps): React.ReactElement {
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
    return <p role="alert">Filters could not be loaded.</p>;
  }
  if (module === null) {
    return <p>Loading filters…</p>;
  }
  return <FilterSectionHost module={module} {...props} />;
}
