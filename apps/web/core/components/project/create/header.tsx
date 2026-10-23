/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { Controller, useFormContext } from "react-hook-form";
// pi dash imports
import { ETabIndices } from "@pi-dash/constants";
import { EmojiPicker, EmojiIconPickerTypes, Logo } from "@pi-dash/propel/emoji-icon-picker";
import { CloseIcon } from "@pi-dash/propel/icons";
// pi dash types
import type { IProject } from "@pi-dash/types";
// pi dash ui
import { getTabIndex } from "@pi-dash/utils";
// pi dash web imports
import { ProjectTemplateSelect } from "@/pi-dash-web/components/projects/create/template-select";

type Props = {
  handleClose: () => void;
  isMobile?: boolean;
  handleFormOnChange?: () => void;
  isClosable?: boolean;
  handleTemplateSelect?: () => void;
  showActionButtons?: boolean;
};

function ProjectCreateHeader(props: Props) {
  const {
    handleClose,
    isMobile = false,
    handleFormOnChange,
    isClosable = true,
    handleTemplateSelect,
    showActionButtons = true,
  } = props;
  const { control, setValue } = useFormContext<IProject>();

  const [isOpen, setIsOpen] = useState(false);
  const { getIndex } = getTabIndex(ETabIndices.PROJECT_CREATE, isMobile);

  return (
    <div className="flex w-full items-center justify-between gap-3 px-3 pt-3">
      <div className="flex items-center gap-3">
        <Controller
          name="logo_props"
          control={control}
          render={({ field: { value, onChange } }) => (
            <EmojiPicker
              iconType="material"
              isOpen={isOpen}
              handleToggle={(val: boolean) => setIsOpen(val)}
              className="flex items-center justify-center"
              buttonClassName="flex items-center justify-center"
              label={
                <span className="grid h-11 w-11 place-items-center rounded-md border border-subtle bg-layer-2">
                  <Logo logo={value} size={20} />
                </span>
              }
              onChange={(val: any) => {
                let logoValue = {};

                if (val?.type === "emoji")
                  logoValue = {
                    value: val.value,
                  };
                else if (val?.type === "icon") logoValue = val.value;

                const newLogoProps = {
                  in_use: val?.type,
                  [val?.type]: logoValue,
                };
                setValue("logo_props", newLogoProps, {
                  shouldDirty: true,
                });
                onChange(newLogoProps);
                handleFormOnChange?.();
                setIsOpen(false);
              }}
              defaultIconColor={value?.in_use && value.in_use === "icon" ? value.icon?.color : undefined}
              defaultOpen={
                value?.in_use && value.in_use === "emoji" ? EmojiIconPickerTypes.EMOJI : EmojiIconPickerTypes.ICON
              }
            />
          )}
        />
        {showActionButtons && <ProjectTemplateSelect onClick={handleTemplateSelect} />}
      </div>
      {isClosable && (
        <button
          type="button"
          onClick={handleClose}
          tabIndex={getIndex("close")}
          className="rounded-sm p-2 text-secondary hover:bg-layer-transparent-hover hover:text-primary"
        >
          <CloseIcon className="h-5 w-5" />
        </button>
      )}
    </div>
  );
}

export default ProjectCreateHeader;
