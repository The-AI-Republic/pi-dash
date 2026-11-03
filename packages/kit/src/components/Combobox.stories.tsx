// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import * as React from "react";
import { Button } from "./Button";
import { Combobox, type ComboboxOption } from "./Combobox";
import { IconButton } from "./IconButton";

const PRIORITIES: ComboboxOption[] = [
  { id: "urgent", label: "Urgent" },
  { id: "high", label: "High" },
  { id: "medium", label: "Medium" },
  { id: "low", label: "Low" },
  { id: "none", label: "None" },
];

function SingleDemo(): React.ReactElement {
  const [value, setValue] = React.useState<string | null>("high");
  const label = PRIORITIES.find((option) => option.id === value)?.label ?? "Select priority";
  return (
    <Combobox
      trigger={<Button>{label}</Button>}
      options={PRIORITIES}
      value={value}
      onSelect={setValue}
      listLabel="Priorities"
      emptyText="No matching results"
    />
  );
}

function MultiDemo(): React.ReactElement {
  const [value, setValue] = React.useState<readonly string[]>(["high"]);
  return (
    <Combobox
      trigger={<Button>{value.length === 0 ? "Assignees" : `${value.length} selected`}</Button>}
      options={PRIORITIES}
      value={value}
      multiple
      onSelect={(id) => setValue(value.includes(id) ? value.filter((entry) => entry !== id) : [...value, id])}
      listLabel="Assignees"
      emptyText="No matching results"
    />
  );
}

export const Single = (): React.ReactElement => <SingleDemo />;

export const Multi = (): React.ReactElement => <MultiDemo />;

export const Triggers = (): React.ReactElement => (
  <div style={{ display: "flex", gap: 8 }}>
    <Combobox
      trigger={<Button variant="primary">Primary</Button>}
      options={PRIORITIES}
      value={null}
      onSelect={() => undefined}
      listLabel="Priorities"
      emptyText="No matching results"
    />
    <Combobox
      trigger={
        <IconButton label="Pick priority">
          <span aria-hidden="true">!</span>
        </IconButton>
      }
      options={PRIORITIES}
      value={null}
      onSelect={() => undefined}
      listLabel="Priorities"
      emptyText="No matching results"
    />
    <Combobox
      trigger={<Button disabled>Unavailable</Button>}
      options={PRIORITIES}
      value={null}
      onSelect={() => undefined}
      listLabel="Priorities"
      emptyText="No matching results"
      disabled
    />
  </div>
);

export const Loading = (): React.ReactElement => (
  <Combobox
    trigger={<Button>States</Button>}
    options={[]}
    value={null}
    onSelect={() => undefined}
    listLabel="States"
    emptyText="No matching results"
    loading
    loadingText="Loading states"
    open
  />
);
