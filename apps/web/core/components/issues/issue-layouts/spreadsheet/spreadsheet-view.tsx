/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import React, { useRef } from "react";
import { observer } from "mobx-react";
// pi dash constants
import { SPREADSHEET_SELECT_GROUP, SPREADSHEET_PROPERTY_LIST } from "@pi-dash/constants";
// types
import type { TIssue, IIssueDisplayFilterOptions, IIssueDisplayProperties } from "@pi-dash/types";
import { EIssueLayoutTypes } from "@pi-dash/types";
// components
import { MultipleSelectGroup } from "@/components/core/multiple-select";
// hooks
import { useProject } from "@/hooks/store/use-project";
// pi dash web components
import { IssueBulkOperationsRoot } from "@/pi-dash-web/components/issues/bulk-operations";
// pi dash web hooks
import { useBulkOperationStatus } from "@/pi-dash-web/hooks/use-bulk-operation-status";
// local imports
import type { TRenderQuickActions } from "../list/list-view-types";
import { QuickAddIssueRoot, SpreadsheetAddIssueButton } from "../quick-add";
import { SpreadsheetTable } from "./spreadsheet-table";

type Props = {
  displayProperties: IIssueDisplayProperties;
  displayFilters: IIssueDisplayFilterOptions;
  handleDisplayFilterUpdate: (data: Partial<IIssueDisplayFilterOptions>) => void;
  issueIds: string[] | undefined;
  quickActions: TRenderQuickActions;
  updateIssue: ((projectId: string | null, issueId: string, data: Partial<TIssue>) => Promise<void>) | undefined;
  openIssuesListModal?: (() => void) | null;
  quickAddCallback?: (projectId: string | null | undefined, data: TIssue) => Promise<TIssue | undefined>;
  canEditProperties: (projectId: string | undefined) => boolean;
  canLoadMoreIssues: boolean;
  loadMoreIssues: () => void;
  enableQuickCreateIssue?: boolean;
  disableIssueCreation?: boolean;
  isWorkspaceLevel?: boolean;
  isEpic?: boolean;
};

export const SpreadsheetView = observer(function SpreadsheetView(props: Props) {
  const {
    displayProperties,
    displayFilters,
    handleDisplayFilterUpdate,
    issueIds,
    quickActions,
    updateIssue,
    quickAddCallback,
    canEditProperties,
    enableQuickCreateIssue,
    disableIssueCreation,
    canLoadMoreIssues,
    loadMoreIssues,
    isWorkspaceLevel = false,
    isEpic = false,
  } = props;
  // refs
  const containerRef = useRef<HTMLTableElement | null>(null);
  const portalRef = useRef<HTMLDivElement | null>(null);
  // store hooks
  const { currentProjectDetails } = useProject();
  // pi dash web hooks
  const isBulkOperationsEnabled = useBulkOperationStatus();

  const isEstimateEnabled: boolean = currentProjectDetails?.estimate !== null;

  const spreadsheetColumnsList = isWorkspaceLevel
    ? SPREADSHEET_PROPERTY_LIST
    : SPREADSHEET_PROPERTY_LIST.filter((property) => {
        if (property === "cycle" && !currentProjectDetails?.cycle_view) return false;
        if (property === "modules" && !currentProjectDetails?.module_view) return false;
        return true;
      });

  if (!issueIds || issueIds.length === 0) return <></>;
  return (
    <div className="relative flex h-full w-full flex-col overflow-x-hidden bg-layer-1 whitespace-nowrap text-secondary">
      <div ref={portalRef} className="spreadsheet-menu-portal" />
      <MultipleSelectGroup
        containerRef={containerRef}
        entities={{
          [SPREADSHEET_SELECT_GROUP]: issueIds,
        }}
        disabled={!isBulkOperationsEnabled || isEpic}
      >
        {(helpers) => (
          <>
            <div ref={containerRef} className="vertical-scrollbar horizontal-scrollbar scrollbar-lg h-full w-full">
              <SpreadsheetTable
                displayProperties={displayProperties}
                displayFilters={displayFilters}
                handleDisplayFilterUpdate={handleDisplayFilterUpdate}
                issueIds={issueIds}
                isEstimateEnabled={isEstimateEnabled}
                portalElement={portalRef}
                quickActions={quickActions}
                updateIssue={updateIssue}
                canEditProperties={canEditProperties}
                containerRef={containerRef}
                canLoadMoreIssues={canLoadMoreIssues}
                loadMoreIssues={loadMoreIssues}
                spreadsheetColumnsList={spreadsheetColumnsList}
                selectionHelpers={helpers}
                isEpic={isEpic}
              />
            </div>
            <div className="border-t border-subtle">
              <div className="sticky bottom-0 left-0 z-5">
                {enableQuickCreateIssue && !disableIssueCreation && (
                  <QuickAddIssueRoot
                    layout={EIssueLayoutTypes.SPREADSHEET}
                    QuickAddButton={SpreadsheetAddIssueButton}
                    quickAddCallback={quickAddCallback}
                    isEpic={isEpic}
                  />
                )}
              </div>
            </div>
            <IssueBulkOperationsRoot selectionHelpers={helpers} />
          </>
        )}
      </MultipleSelectGroup>
    </div>
  );
});
