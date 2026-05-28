/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

// pi dash imports
import { AiIcon, CommentFillIcon, PageIcon, StickyNoteIcon, WorkItemsIcon } from "@pi-dash/propel/icons";
import type { ISvgIcons } from "@pi-dash/propel/icons";
// types
import type { TTourSteps } from "./root";

const sidebarOptions: {
  key: TTourSteps;
  label: string;
  Icon: React.FC<ISvgIcons>;
}[] = [
  {
    key: "work-items",
    label: "Work items",
    Icon: WorkItemsIcon,
  },
  {
    key: "runners",
    label: "Runners",
    Icon: AiIcon,
  },
  {
    key: "chat",
    label: "Runner chat",
    Icon: CommentFillIcon,
  },
  {
    key: "workpads",
    label: "Workpads",
    Icon: StickyNoteIcon,
  },
  {
    key: "pages",
    label: "Pages",
    Icon: PageIcon,
  },
];

type Props = {
  step: TTourSteps;
  setStep: React.Dispatch<React.SetStateAction<TTourSteps>>;
};

export function TourSidebar({ step, setStep }: Props) {
  return (
    <div className="col-span-3 hidden bg-surface-2 p-8 lg:block">
      <h3 className="text-16 font-medium">
        Let{"'"}s get started!
        <br />
        Get more out of Pi Dash.
      </h3>
      <div className="mt-8 space-y-5">
        {sidebarOptions.map((option) => (
          <button
            key={option.key}
            type="button"
            className={`flex w-full cursor-pointer items-center gap-2 border-l-[3px] py-0.5 pr-2 pl-3 text-left text-13 font-medium capitalize ${
              step === option.key ? "border-accent-strong text-accent-primary" : "border-transparent text-secondary"
            }`}
            onClick={() => setStep(option.key)}
          >
            <option.Icon className="h-4 w-4" aria-hidden="true" />
            {option.label}
          </button>
        ))}
      </div>
    </div>
  );
}
