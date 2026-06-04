/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useRef } from "react";
import { observer } from "mobx-react";
import { useParams } from "next/navigation";
import type { SubmitHandler } from "react-hook-form";
import { Controller, useForm } from "react-hook-form";
import { useOutsideClickDetector } from "@pi-dash/hooks";
import { useTranslation } from "@pi-dash/i18n";
// pi dash helpers
// pi dash ui
import { FavoriteFolderIcon } from "@pi-dash/propel/icons";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import { Input } from "@pi-dash/ui";
// hooks
import { useFavorite } from "@/hooks/store/use-favorite";

type TForm = {
  name: string;
  entity_type: string;
  parent: string | null;
  project_id: string | null;
  is_folder: boolean;
};
type TProps = {
  setCreateNewFolder: (value: boolean | string | null) => void;
  actionType: "create" | "rename";
  defaultName?: string;
  favoriteId?: string;
};
export const NewFavoriteFolder = observer(function NewFavoriteFolder(props: TProps) {
  const { setCreateNewFolder, actionType, defaultName, favoriteId } = props;
  const { t } = useTranslation();
  const { workspaceSlug } = useParams();
  const { addFavorite, updateFavorite, existingFolders } = useFavorite();

  // ref
  const ref = useRef(null);

  // form info
  const { handleSubmit, control, setValue, setFocus } = useForm<TForm>({
    reValidateMode: "onChange",
    defaultValues: {
      name: defaultName,
    },
  });

  const handleAddNewFolder: SubmitHandler<TForm> = (formData) => {
    if (existingFolders.includes(formData.name))
      return setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Error"),
        message: t("Folder already exists"),
      });
    formData = {
      entity_type: "folder",
      is_folder: true,
      name: formData.name.trim(),
      parent: null,
      project_id: null,
    };

    if (formData.name === "")
      return setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Error"),
        message: t("Folder name cannot be empty"),
      });

    addFavorite(workspaceSlug.toString(), formData)
      .then(() => {
        setToast({
          type: TOAST_TYPE.SUCCESS,
          title: t("Success"),
          message: t("Favorite created successfully"),
        });
        return;
      })
      .catch(() => {
        setToast({
          type: TOAST_TYPE.ERROR,
          title: t("Error"),
          message: t("Something went wrong"),
        });
      });
    setCreateNewFolder(false);
    setValue("name", "");
  };

  const handleRenameFolder: SubmitHandler<TForm> = (formData) => {
    if (!favoriteId) return;
    if (existingFolders.includes(formData.name))
      return setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Error"),
        message: t("Folder already exists"),
      });
    const payload = {
      name: formData.name.trim(),
    };

    if (formData.name.trim() === "")
      return setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Error"),
        message: t("Folder name cannot be empty"),
      });

    updateFavorite(workspaceSlug.toString(), favoriteId, payload)
      .then(() => {
        setToast({
          type: TOAST_TYPE.SUCCESS,
          title: t("Success"),
          message: t("Favorite updated successfully"),
        });
        return;
      })
      .catch(() => {
        setToast({
          type: TOAST_TYPE.ERROR,
          title: t("Error"),
          message: t("Something went wrong"),
        });
      });
    setCreateNewFolder(false);
    setValue("name", "");
  };

  useEffect(() => {
    setFocus("name");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  useOutsideClickDetector(ref, () => {
    setCreateNewFolder(false);
  });
  return (
    <div className="flex items-center gap-1.5 px-2 py-[1px]" ref={ref}>
      <FavoriteFolderIcon className="size-4" />
      <form onSubmit={handleSubmit(actionType === "create" ? handleAddNewFolder : handleRenameFolder)}>
        <Controller
          name="name"
          control={control}
          rules={{ required: true }}
          render={({ field }) => (
            <Input
              className="w-full"
              placeholder={t("New folder")}
              aria-label={t("Enter folder name")}
              {...field}
            />
          )}
        />
      </form>
    </div>
  );
});
