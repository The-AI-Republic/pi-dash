// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Modal work-item type picker (NEWFRONT-984, ISS-129 type half). The
// create dialog renders this when the cloud bootstrap registers the
// type extension; template and extra-property slots stay absent until
// the Part 5 model lands. Options come from the project type list, so a
// project without types offers "No type" only.
import { Button, Combobox, type ComboboxOption } from "@pidash/kit";
import { useQuery } from "@tanstack/react-query";
import * as React from "react";

import type { IssueTypeExtensionProps } from "../create/issueTypeSeam.js";
import { editionOptionsKey } from "./EditionFilterSections.js";
import { getEditionIssuesProviders } from "./providers.js";

const NO_TYPE_ID = "";
const NO_TYPE_LABEL = "No type";

/** Project types for pickers outside the filter builder (registration-gated). */
export function useProjectWorkItemTypes(workspaceSlug: string, projectId: string) {
  const provider = getEditionIssuesProviders().workItemTypes;
  return useQuery({
    queryKey: editionOptionsKey(workspaceSlug, projectId, "types"),
    queryFn: () => provider!({ workspaceSlug, projectId }),
    enabled: provider !== undefined,
  });
}

function toComboboxOption(id: string, label: string): ComboboxOption {
  return { id, label, searchText: label };
}

export function IssueTypePicker({
  workspaceSlug,
  projectId,
  disabled,
  typeId,
  onTypeChange,
}: IssueTypeExtensionProps): React.ReactElement {
  const types = useProjectWorkItemTypes(workspaceSlug, projectId);
  const options = React.useMemo(
    () => [
      toComboboxOption(NO_TYPE_ID, NO_TYPE_LABEL),
      ...(types.data ?? []).map((option) => toComboboxOption(option.id, option.name)),
    ],
    [types.data]
  );
  const current = options.find((option) => option.id === (typeId ?? NO_TYPE_ID)) ?? options[0]!;
  return (
    <Combobox
      trigger={
        <Button variant="secondary" disabled={disabled}>
          <span className="truncate">{current.label}</span>
        </Button>
      }
      options={options}
      value={typeId ?? NO_TYPE_ID}
      onSelect={(id) => onTypeChange(id === NO_TYPE_ID ? null : id)}
      listLabel="Issue types"
      searchLabel="Search issue types"
      emptyText="No work-item types"
      loading={types.isPending}
      loadingText="Loading issue types…"
      errorText={types.isError ? "Issue types failed to load." : undefined}
      disabled={disabled}
    />
  );
}
