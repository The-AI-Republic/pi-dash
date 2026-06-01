/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect } from "react";
import { observer } from "mobx-react";
import { Controller, useForm } from "react-hook-form";
// pi dash imports
import { useTranslation } from "@pi-dash/i18n";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import type { IUserEmailNotificationSettings } from "@pi-dash/types";
import { ToggleSwitch } from "@pi-dash/ui";
// components
import { SettingsControlItem } from "@/components/settings/control-item";
// services
import { UserService } from "@/services/user.service";

type Props = {
  data: IUserEmailNotificationSettings;
};

// services
const userService = new UserService();

export const NotificationsProfileSettingsForm = observer(function NotificationsProfileSettingsForm(props: Props) {
  const { data } = props;
  // translation
  const { t } = useTranslation();
  // form data
  const { control, reset } = useForm<IUserEmailNotificationSettings>({
    defaultValues: {
      ...data,
    },
  });

  const handleSettingChange = async (key: keyof IUserEmailNotificationSettings, value: boolean) => {
    try {
      await userService.updateCurrentUserEmailNotificationSettings({
        [key]: value,
      });
      setToast({
        title: t("Success"),
        type: TOAST_TYPE.SUCCESS,
        message: t("Email notification setting updated successfully"),
      });
    } catch (_error) {
      setToast({
        title: t("Error"),
        type: TOAST_TYPE.ERROR,
        message: t("Failed to update email notification setting"),
      });
    }
  };

  useEffect(() => {
    reset(data);
  }, [reset, data]);

  return (
    <div className="flex flex-col gap-y-1">
      <SettingsControlItem
        title={t("Property changes")}
        description={t("Notify me when work items' properties like assignees, priority, estimates or anything else changes.")}
        control={
          <Controller
            control={control}
            name="property_change"
            render={({ field: { value, onChange } }) => (
              <ToggleSwitch
                value={value}
                onChange={(newValue) => {
                  onChange(newValue);
                  handleSettingChange("property_change", newValue);
                }}
                size="sm"
              />
            )}
          />
        }
      />
      <SettingsControlItem
        title={t("State change")}
        description={t("Notify me when the work items moves to a different state")}
        control={
          <Controller
            control={control}
            name="state_change"
            render={({ field: { value, onChange } }) => (
              <ToggleSwitch
                value={value}
                onChange={(newValue) => {
                  onChange(newValue);
                  handleSettingChange("state_change", newValue);
                }}
                size="sm"
              />
            )}
          />
        }
      />
      <div className="border-l-3 border-subtle-1 pl-3">
        <SettingsControlItem
          title={t("Work item completed")}
          description={t("Notify me only when a work item is completed")}
          control={
            <Controller
              control={control}
              name="issue_completed"
              render={({ field: { value, onChange } }) => (
                <ToggleSwitch
                  value={value}
                  onChange={(newValue) => {
                    onChange(newValue);
                    handleSettingChange("issue_completed", newValue);
                  }}
                  size="sm"
                />
              )}
            />
          }
        />
      </div>
      <SettingsControlItem
        title={t("Comments")}
        description={t("Notify me when someone leaves a comment on the work item")}
        control={
          <Controller
            control={control}
            name="comment"
            render={({ field: { value, onChange } }) => (
              <ToggleSwitch
                value={value}
                onChange={(newValue) => {
                  onChange(newValue);
                  handleSettingChange("comment", newValue);
                }}
                size="sm"
              />
            )}
          />
        }
      />
      <SettingsControlItem
        title={t("Mentions")}
        description={t("Notify me only when someone mentions me in the comments or description")}
        control={
          <Controller
            control={control}
            name="mention"
            render={({ field: { value, onChange } }) => (
              <ToggleSwitch
                value={value}
                onChange={(newValue) => {
                  onChange(newValue);
                  handleSettingChange("mention", newValue);
                }}
                size="sm"
              />
            )}
          />
        }
      />
    </div>
  );
});
