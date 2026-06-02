/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import Link from "next/link";
import { useSearchParams } from "next/navigation";
import { Controller, useForm } from "react-hook-form";
// icons
import { CircleCheck } from "lucide-react";
// pi dash imports
import { useTranslation } from "@pi-dash/i18n";
import { Button, getButtonStyling } from "@pi-dash/propel/button";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import { Input } from "@pi-dash/ui";
import { cn, checkEmailValidity } from "@pi-dash/utils";
// hooks
import useTimer from "@/hooks/use-timer";
// services
import { AuthService } from "@/services/auth.service";
// local components
import { FormContainer } from "./common/container";
import { AuthFormHeader } from "./common/header";

type TForgotPasswordFormValues = {
  email: string;
};

const defaultValues: TForgotPasswordFormValues = {
  email: "",
};

// services
const authService = new AuthService();

export const ForgotPasswordForm = observer(function ForgotPasswordForm() {
  // search params
  const searchParams = useSearchParams();
  const email = searchParams.get("email");
  // pi dash hooks
  const { t } = useTranslation();
  // timer
  const { timer: resendTimerCode, setTimer: setResendCodeTimer } = useTimer(0);

  // form info
  const {
    control,
    formState: { errors, isSubmitting, isValid },
    handleSubmit,
  } = useForm<TForgotPasswordFormValues>({
    defaultValues: {
      ...defaultValues,
      email: email?.toString() ?? "",
    },
  });

  const handleForgotPassword = async (formData: TForgotPasswordFormValues) => {
    await authService
      .sendResetPasswordLink({
        email: formData.email,
      })
      .then(() => {
        setToast({
          type: TOAST_TYPE.SUCCESS,
          title: t("Email sent"),
          message: t("Check your inbox for a link to reset your password. If it doesn't appear within a few minutes, check your spam folder."),
        });
        setResendCodeTimer(30);
      })
      .catch((err) => {
        setToast({
          type: TOAST_TYPE.ERROR,
          title: t("Error!"),
          message: err?.error ?? t("Something went wrong. Please try again."),
        });
      });
  };

  return (
    <FormContainer>
      <AuthFormHeader title="Reset password" description="Regain access to your account." />
      <form onSubmit={handleSubmit(handleForgotPassword)} className="space-y-4">
        <div className="space-y-1">
          <label className="text-13 font-medium text-tertiary" htmlFor="email">
            {t("Email")}
          </label>
          <Controller
            control={control}
            name="email"
            rules={{
              required: t("Email is required"),
              validate: (value) => checkEmailValidity(value) || t("Email is invalid"),
            }}
            render={({ field: { value, onChange, ref } }) => (
              <Input
                id="email"
                name="email"
                type="email"
                value={value}
                onChange={onChange}
                ref={ref}
                hasError={Boolean(errors.email)}
                placeholder={t("name@company.com")}
                className="h-10 w-full border border-strong !bg-surface-1 pr-12 placeholder:text-placeholder"
                autoComplete="off"
                disabled={resendTimerCode > 0}
              />
            )}
          />
          {resendTimerCode > 0 && (
            <p className="flex w-full items-start gap-1 px-1 text-11 font-medium text-success-primary">
              <CircleCheck height={12} width={12} className="mt-0.5" />
              {t("We sent the reset link to your email address")}
            </p>
          )}
        </div>
        <Button
          type="submit"
          variant="primary"
          className="w-full"
          size="xl"
          disabled={!isValid}
          loading={isSubmitting || resendTimerCode > 0}
        >
          {resendTimerCode > 0
            ? t("Resend in {seconds} seconds", { seconds: resendTimerCode })
            : t("Send reset link")}
        </Button>
        <Link href="/" className={cn("w-full", getButtonStyling("link", "lg"))}>
          {t("Back to sign in")}
        </Link>
      </form>
    </FormContainer>
  );
});
