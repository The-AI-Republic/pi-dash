// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { ApiError } from "@pidash/api-client";
import { afterEach, describe, expect, it, vi } from "vitest";

import type {
  SpaceAuthCardProps,
  SpaceAuthCheckAnswer,
  SpaceAuthInstanceCapabilities,
  SpaceAuthServices,
} from "../index.js";
import { SpaceAuthCard } from "../index.js";

const FULL: SpaceAuthInstanceCapabilities = {
  smtpConfigured: true,
  magicLoginEnabled: true,
  emailPasswordEnabled: true,
  googleEnabled: false,
  githubEnabled: false,
  gitlabEnabled: false,
  giteaEnabled: false,
};

const PASSWORD_ONLY: SpaceAuthInstanceCapabilities = {
  ...FULL,
  smtpConfigured: false,
  magicLoginEnabled: false,
};

const NO_METHODS: SpaceAuthInstanceCapabilities = {
  ...FULL,
  smtpConfigured: false,
  magicLoginEnabled: false,
  emailPasswordEnabled: false,
};

interface FakeServices extends SpaceAuthServices {
  checked: string[];
  generated: string[];
}

function fakeServices(): FakeServices {
  const fake = {
    checked: [] as string[],
    generated: [] as string[],
    checkEmail: async (email: string): Promise<SpaceAuthCheckAnswer> => {
      fake.checked.push(email);
      return { existing: false, status: "CREDENTIAL" };
    },
    generateCode: async (email: string): Promise<void> => {
      fake.generated.push(email);
    },
    readCsrfToken: async (): Promise<string> => "csrf-token",
  };
  return fake;
}

function renderCard(props: Partial<SpaceAuthCardProps>, services: SpaceAuthServices = fakeServices()) {
  return render(<SpaceAuthCard instance={FULL} services={services} {...props} />);
}

function card(): HTMLElement {
  return screen.getByTestId("space-auth-card");
}

function emailField(): HTMLElement {
  return screen.getByLabelText("Email address");
}

function emailSubmit(): HTMLElement {
  return screen.getByRole("button", { name: "Next" });
}

function submitEmail(address: string): void {
  fireEvent.change(emailField(), { target: { value: address } });
  fireEvent.submit(screen.getByTestId("space-auth-email-form"));
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("email step", () => {
  it("opens on the email step in sign-up mode with its heading and consent", () => {
    renderCard({});
    expect(card()).toHaveAttribute("data-step", "email");
    expect(card()).toHaveAttribute("data-mode", "sign-up");
    expect(card()).toHaveAttribute("aria-label", "Sign up");
    expect(screen.getByTestId("space-auth-header")).toHaveTextContent("Join the conversation");
    expect(screen.getByTestId("space-auth-header")).toHaveTextContent(
      "Create an account to vote and comment on public boards."
    );
    const consent = screen.getByTestId("space-auth-consent");
    expect(consent).toHaveTextContent("By creating an account, you agree to our");
    expect(within(consent).getByRole("link", { name: "Terms of Service" })).toHaveAttribute(
      "href",
      "https://airepublic.com/terms"
    );
    expect(within(consent).getByRole("link", { name: "Privacy Policy" })).toHaveAttribute(
      "href",
      "https://airepublic.com/privacy"
    );
    expect(screen.queryByTestId("space-auth-banner")).toBeNull();
  });

  it("honors legal href overrides", () => {
    renderCard({ termsHref: "https://example.test/t", privacyHref: "https://example.test/p" });
    const consent = screen.getByTestId("space-auth-consent");
    expect(within(consent).getByRole("link", { name: "Terms of Service" })).toHaveAttribute(
      "href",
      "https://example.test/t"
    );
    expect(within(consent).getByRole("link", { name: "Privacy Policy" })).toHaveAttribute(
      "href",
      "https://example.test/p"
    );
  });

  it("blocks empty and malformed addresses without an error", () => {
    renderCard({});
    expect(emailSubmit()).toBeDisabled();
    expect(screen.queryByText("That email address doesn't look right.")).toBeNull();
    fireEvent.change(emailField(), { target: { value: "not-an-email" } });
    expect(emailSubmit()).toBeDisabled();
    expect(screen.queryByText("That email address doesn't look right.")).toBeNull();
  });

  it("flags malformed addresses after blur and hides the notice while focused", () => {
    renderCard({});
    fireEvent.change(emailField(), { target: { value: "not-an-email" } });
    fireEvent.blur(emailField());
    expect(screen.getByText("That email address doesn't look right.")).toBeInTheDocument();
    fireEvent.focus(emailField());
    expect(screen.queryByText("That email address doesn't look right.")).toBeNull();
    fireEvent.change(emailField(), { target: { value: "ada@example.com" } });
    fireEvent.blur(emailField());
    expect(screen.queryByText("That email address doesn't look right.")).toBeNull();
    expect(emailSubmit()).toBeEnabled();
  });

  it("clears the field from its clear control", () => {
    renderCard({});
    expect(screen.queryByRole("button", { name: "Clear email" })).toBeNull();
    fireEvent.change(emailField(), { target: { value: "ada@example.com" } });
    fireEvent.click(screen.getByRole("button", { name: "Clear email" }));
    expect(emailField()).toHaveValue("");
    expect(screen.queryByRole("button", { name: "Clear email" })).toBeNull();
  });

  it("prefills the address it is handed", () => {
    renderCard({ initialEmail: "Ada@Example.com" });
    expect(emailField()).toHaveValue("Ada@Example.com");
  });

  it("never checks an invalid address", () => {
    const services = fakeServices();
    renderCard({}, services);
    fireEvent.change(emailField(), { target: { value: "not-an-email" } });
    fireEvent.submit(screen.getByTestId("space-auth-email-form"));
    expect(services.checked).toEqual([]);
  });

  it("shows progress while the check is in flight", async () => {
    const gate = deferred<SpaceAuthCheckAnswer>();
    const services = fakeServices();
    services.checkEmail = () => gate.promise;
    renderCard({}, services);
    fireEvent.change(emailField(), { target: { value: "ada@example.com" } });
    fireEvent.submit(screen.getByTestId("space-auth-email-form"));
    expect(emailSubmit()).toBeDisabled();
    expect(emailSubmit()).toHaveAttribute("aria-busy", "true");
    gate.resolve({ existing: false, status: "CREDENTIAL" });
    await screen.findByTestId("space-auth-password-form");
  });

  it("normalizes the checked address", async () => {
    const services = fakeServices();
    renderCard({ instance: PASSWORD_ONLY }, services);
    submitEmail("  Ada@Example.COM ");
    await screen.findByTestId("space-auth-password-form");
    expect(services.checked).toEqual(["ada@example.com"]);
  });
});

describe("method routing", () => {
  it("routes new addresses with mail on to the code step and generates a code", async () => {
    const services = fakeServices();
    services.checkEmail = async (email) => {
      services.checked.push(email);
      return { existing: false, status: "MAGIC_CODE" };
    };
    renderCard({}, services);
    submitEmail("ada@example.com");
    await screen.findByTestId("space-auth-code-form");
    expect(card()).toHaveAttribute("data-step", "code");
    expect(card()).toHaveAttribute("data-mode", "sign-up");
    expect(services.generated).toEqual(["ada@example.com"]);
  });

  it("routes code-backed known addresses to the code step in sign-in mode", async () => {
    const services = fakeServices();
    services.checkEmail = async () => ({ existing: true, status: "MAGIC_CODE" });
    renderCard({}, services);
    submitEmail("ada@example.com");
    await screen.findByTestId("space-auth-code-form");
    expect(card()).toHaveAttribute("data-mode", "sign-in");
    expect(screen.getByTestId("space-auth-header")).toHaveTextContent("Welcome back");
    expect(screen.getByTestId("space-auth-consent")).toHaveTextContent("By signing in, you agree to our");
    expect(services.generated).toEqual(["ada@example.com"]);
  });

  it("routes password accounts to the password step even with mail on", async () => {
    const services = fakeServices();
    services.checkEmail = async () => ({ existing: true, status: "CREDENTIAL" });
    renderCard({}, services);
    submitEmail("ada@example.com");
    await screen.findByTestId("space-auth-password-form");
    expect(card()).toHaveAttribute("data-step", "password");
    expect(card()).toHaveAttribute("data-mode", "sign-in");
    expect(services.generated).toEqual([]);
  });

  it("routes to the password step when mail is off", async () => {
    renderCard({ instance: PASSWORD_ONLY });
    submitEmail("ada@example.com");
    await screen.findByTestId("space-auth-password-form");
    expect(card()).toHaveAttribute("data-mode", "sign-up");
  });

  it("explains itself in sign-up mode when no method is enabled", async () => {
    renderCard({ instance: NO_METHODS });
    submitEmail("ada@example.com");
    await screen.findByTestId("space-auth-banner");
    expect(card()).toHaveAttribute("data-step", "email");
    expect(card()).toHaveAttribute("data-mode", "sign-up");
    expect(screen.getByTestId("space-auth-banner")).toHaveTextContent(
      "Signing in isn't available on this instance. Ask your administrator to enable a sign-in method."
    );
    expect(emailField()).toHaveValue("ada@example.com");
  });

  it("explains itself in sign-in mode when no method is enabled", async () => {
    const services = fakeServices();
    services.checkEmail = async () => ({ existing: true, status: "CREDENTIAL" });
    renderCard({ instance: NO_METHODS }, services);
    submitEmail("ada@example.com");
    await screen.findByTestId("space-auth-banner");
    expect(card()).toHaveAttribute("data-mode", "sign-in");
  });

  it("banners a refused check in place", async () => {
    const services = fakeServices();
    services.checkEmail = () => Promise.reject(new ApiError({ code: "5005", message: "bad" }));
    renderCard({}, services);
    submitEmail("ada@example.com");
    await screen.findByTestId("space-auth-banner");
    expect(card()).toHaveAttribute("data-step", "email");
    expect(screen.getByTestId("space-auth-banner")).toHaveTextContent("That email address doesn't look right.");
  });

  it("stays silent on a throttled check", async () => {
    const services = fakeServices();
    services.checkEmail = () => Promise.reject(new ApiError({ code: "5900", message: "throttled" }));
    renderCard({}, services);
    submitEmail("ada@example.com");
    await waitForEmailSettled();
    expect(screen.queryByTestId("space-auth-banner")).toBeNull();
    expect(emailField()).toHaveValue("ada@example.com");
  });

  it("explains a transport failure on the check", async () => {
    const services = fakeServices();
    services.checkEmail = () => Promise.reject(new Error("down"));
    renderCard({}, services);
    submitEmail("ada@example.com");
    await screen.findByTestId("space-auth-banner");
    expect(screen.getByTestId("space-auth-banner")).toHaveTextContent(
      "The request failed. Check your connection and try again."
    );
  });

  it("supersedes the banner once a check routes", async () => {
    const services = fakeServices();
    let calls = 0;
    services.checkEmail = async (email) => {
      calls += 1;
      services.checked.push(email);
      if (calls === 1) throw new ApiError({ code: "5005", message: "bad" });
      return { existing: false, status: "CREDENTIAL" };
    };
    renderCard({ instance: PASSWORD_ONLY }, services);
    submitEmail("ada@example.com");
    await screen.findByTestId("space-auth-banner");
    fireEvent.submit(screen.getByTestId("space-auth-email-form"));
    await screen.findByTestId("space-auth-password-form");
    expect(services.checked).toEqual(["ada@example.com", "ada@example.com"]);
    expect(screen.queryByTestId("space-auth-banner")).toBeNull();
  });
});

async function waitForEmailSettled(): Promise<void> {
  await screen.findByRole("button", { name: "Next" });
  expect(emailSubmit()).toBeEnabled();
}

describe("code step", () => {
  async function renderCodeStep(
    props: Partial<SpaceAuthCardProps> = {},
    services: FakeServices = fakeServices()
  ): Promise<FakeServices> {
    services.checkEmail = async (email) => {
      services.checked.push(email);
      return { existing: false, status: "MAGIC_CODE" };
    };
    renderCard(props, services);
    fireEvent.change(emailField(), { target: { value: "ada@example.com" } });
    fireEvent.submit(screen.getByTestId("space-auth-email-form"));
    await screen.findByTestId("space-auth-code-form");
    return services;
  }

  it("renders the locked address, guidance, resend and a gated submit", async () => {
    const services = fakeServices();
    services.readCsrfToken = () => deferred<string>().promise;
    await renderCodeStep({ nextPath: "/issues/anchor" }, services);
    const form = screen.getByTestId("space-auth-code-form") as HTMLFormElement;
    expect(form.getAttribute("method")).toBe("post");
    expect(form.getAttribute("action")).toBe("/auth/spaces/magic-sign-up/");
    expect(screen.getByLabelText("Email address")).toHaveValue("ada@example.com");
    expect(screen.getByLabelText("Email address")).toBeDisabled();
    expect(screen.getByText("Enter the code from the email we just sent.")).toBeInTheDocument();
    expect(form.querySelector('input[name="email"]')).toHaveAttribute("value", "ada@example.com");
    expect(form.querySelector('input[name="next_path"]')).toHaveAttribute("value", "/issues/anchor");
    expect(form.querySelector('input[name="csrfmiddlewaretoken"]')).toHaveAttribute("value", "");
    const submit = screen.getByRole("button", { name: "Verify code" });
    expect(submit).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Email code"), { target: { value: "123456" } });
    expect(submit).toBeDisabled();
  });

  it("omits the return field when handed nothing", async () => {
    await renderCodeStep({});
    const form = screen.getByTestId("space-auth-code-form") as HTMLFormElement;
    expect(form.querySelector('input[name="next_path"]')).toBeNull();
  });

  it("omits the return field when handed an empty address", async () => {
    await renderCodeStep({ nextPath: "" });
    const form = screen.getByTestId("space-auth-code-form") as HTMLFormElement;
    expect(form.querySelector('input[name="next_path"]')).toBeNull();
  });

  it("enables submit once the token lands and a code is typed", async () => {
    const gate = deferred<string>();
    const services = fakeServices();
    services.readCsrfToken = () => gate.promise;
    await renderCodeStep({}, services);
    const submit = screen.getByRole("button", { name: "Verify code" });
    fireEvent.change(screen.getByLabelText("Email code"), { target: { value: "123456" } });
    expect(submit).toBeDisabled();
    gate.resolve("csrf-token");
    await screen.findByTestId("space-auth-code-form");
    expect(submit).toBeEnabled();
  });

  it("banners a failed token read and keeps submit shut", async () => {
    const services = fakeServices();
    services.readCsrfToken = () => Promise.reject(new Error("down"));
    await renderCodeStep({}, services);
    await screen.findByTestId("space-auth-banner");
    expect(screen.getByTestId("space-auth-banner")).toHaveTextContent(
      "The request failed. Check your connection and try again."
    );
    fireEvent.change(screen.getByLabelText("Email code"), { target: { value: "123456" } });
    expect(screen.getByRole("button", { name: "Verify code" })).toBeDisabled();
  });

  it("paces resends behind a cooldown", async () => {
    const services = await renderCodeStep({});
    vi.useFakeTimers();
    const resend = screen.getByTestId("space-auth-resend");
    expect(resend).toHaveTextContent("Send another code");
    expect(resend).toBeEnabled();
    fireEvent.click(resend);
    await act(async () => {});
    expect(services.generated).toEqual(["ada@example.com", "ada@example.com"]);
    expect(resend).toHaveTextContent("Send another code in 5s");
    expect(resend).toBeDisabled();
    act(() => {
      vi.advanceTimersByTime(4000);
    });
    expect(resend).toHaveTextContent("Send another code in 1s");
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(resend).toHaveTextContent("Send another code");
    expect(resend).toBeEnabled();
  });

  it("names the sending state while a resend is in flight", async () => {
    const gate = deferred<void>();
    const services = fakeServices();
    let calls = 0;
    services.generateCode = (email) => {
      calls += 1;
      services.generated.push(email);
      return calls === 1 ? Promise.resolve() : gate.promise;
    };
    await renderCodeStep({}, services);
    const resend = screen.getByTestId("space-auth-resend");
    fireEvent.click(resend);
    await act(async () => {});
    expect(resend).toHaveTextContent("Sending…");
    expect(resend).toBeDisabled();
    gate.resolve();
    await act(async () => {});
    expect(resend).toHaveTextContent(/^Send another code in \ds$/);
  });

  it("banners a capped resend on the step", async () => {
    const services = fakeServices();
    let calls = 0;
    services.generateCode = async (email) => {
      calls += 1;
      services.generated.push(email);
      if (calls > 1) throw new ApiError({ code: "5102", message: "capped" });
    };
    await renderCodeStep({}, services);
    fireEvent.click(screen.getByTestId("space-auth-resend"));
    await screen.findByTestId("space-auth-banner");
    expect(card()).toHaveAttribute("data-step", "code");
    expect(screen.getByTestId("space-auth-banner")).toHaveTextContent(
      "That code has expired. Send another code and try again."
    );
  });

  it("shows progress on submit", async () => {
    await renderCodeStep({});
    fireEvent.change(screen.getByLabelText("Email code"), { target: { value: "123456" } });
    const submit = screen.getByRole("button", { name: "Verify code" });
    expect(submit).toBeEnabled();
    fireEvent.submit(screen.getByTestId("space-auth-code-form"));
    expect(submit).toBeDisabled();
    expect(submit).toHaveAttribute("aria-busy", "true");
  });

  it("clears back to the email step and keeps the mode", async () => {
    await renderCodeStep({});
    fireEvent.click(screen.getByRole("button", { name: "Clear email" }));
    expect(card()).toHaveAttribute("data-step", "email");
    expect(card()).toHaveAttribute("data-mode", "sign-up");
    expect(emailField()).toHaveValue("");
  });
});

describe("password step", () => {
  async function renderPasswordStep(
    props: Partial<SpaceAuthCardProps> = {},
    services: FakeServices = fakeServices()
  ): Promise<FakeServices> {
    renderCard({ instance: PASSWORD_ONLY, ...props }, services);
    fireEvent.change(emailField(), { target: { value: "ada@example.com" } });
    fireEvent.submit(screen.getByTestId("space-auth-email-form"));
    await screen.findByTestId("space-auth-password-form");
    return services;
  }

  it("renders the sign-up shape with a gated submit", async () => {
    await renderPasswordStep({ nextPath: "/issues/anchor" });
    const form = screen.getByTestId("space-auth-password-form") as HTMLFormElement;
    expect(form.getAttribute("method")).toBe("post");
    expect(form.getAttribute("action")).toBe("/auth/spaces/sign-up/");
    expect(screen.getByLabelText("Choose a password")).toBeInTheDocument();
    expect(screen.getByLabelText("Confirm your password")).toBeInTheDocument();
    expect(form.querySelector('input[name="confirm_password"]')).not.toBeNull();
    expect(form.querySelector('input[name="next_path"]')).toHaveAttribute("value", "/issues/anchor");
    expect(screen.queryByTestId("space-auth-strength")).toBeNull();
    expect(screen.getByRole("button", { name: "Get started" })).toBeDisabled();
  });

  it("omits the return field when handed an empty address", async () => {
    await renderPasswordStep({ nextPath: "" });
    const form = screen.getByTestId("space-auth-password-form") as HTMLFormElement;
    expect(form.querySelector('input[name="next_path"]')).toBeNull();
  });

  it("toggles each field independently", async () => {
    await renderPasswordStep({});
    const password = screen.getByLabelText("Choose a password") as HTMLInputElement;
    const confirm = screen.getByLabelText("Confirm your password") as HTMLInputElement;
    expect(password.getAttribute("type")).toBe("password");
    fireEvent.click(screen.getByRole("button", { name: "Show password" }));
    expect(password.getAttribute("type")).toBe("text");
    expect(confirm.getAttribute("type")).toBe("password");
    fireEvent.click(screen.getByRole("button", { name: "Hide password" }));
    expect(password.getAttribute("type")).toBe("password");
    fireEvent.click(screen.getByRole("button", { name: "Show confirmation" }));
    expect(confirm.getAttribute("type")).toBe("text");
    fireEvent.click(screen.getByRole("button", { name: "Hide confirmation" }));
    expect(confirm.getAttribute("type")).toBe("password");
  });

  it("meters weak sign-up passwords and gates submit on strength and match", async () => {
    await renderPasswordStep({});
    const password = screen.getByLabelText("Choose a password");
    const confirm = screen.getByLabelText("Confirm your password");
    const submit = screen.getByRole("button", { name: "Get started" });

    fireEvent.change(password, { target: { value: "abc" } });
    expect(screen.getByTestId("space-auth-strength")).toHaveTextContent("Use 8 or more characters.");
    expect(submit).toBeDisabled();

    fireEvent.change(password, { target: { value: "abcdefgh" } });
    expect(screen.getByTestId("space-auth-strength")).toHaveTextContent(
      "Mix upper-case, lower-case, numbers and symbols."
    );
    expect(screen.getByTestId("space-auth-strength")).toHaveTextContent("Uppercase letter");
    expect(submit).toBeDisabled();

    fireEvent.change(password, { target: { value: "Str0ng!Pass1" } });
    expect(screen.queryByTestId("space-auth-strength")).toBeNull();
    expect(submit).toBeDisabled();

    fireEvent.focus(confirm);
    fireEvent.change(confirm, { target: { value: "Str0ng" } });
    expect(screen.queryByText("Confirmation doesn't match the password.")).toBeNull();
    fireEvent.blur(confirm);
    expect(screen.getByText("Confirmation doesn't match the password.")).toBeInTheDocument();
    expect(submit).toBeDisabled();

    fireEvent.change(confirm, { target: { value: "Str0ng!Pass1" } });
    expect(screen.queryByText("Confirmation doesn't match the password.")).toBeNull();
    expect(submit).toBeEnabled();
  });

  it("flags mismatches while focused once the confirmation catches up in length", async () => {
    await renderPasswordStep({});
    const confirm = screen.getByLabelText("Confirm your password");
    fireEvent.change(screen.getByLabelText("Choose a password"), { target: { value: "Str0ng!Pass1" } });
    fireEvent.focus(confirm);
    fireEvent.change(confirm, { target: { value: "short" } });
    expect(screen.queryByText("Confirmation doesn't match the password.")).toBeNull();
    fireEvent.change(confirm, { target: { value: "Str0ng!Pass2" } });
    expect(screen.getByText("Confirmation doesn't match the password.")).toBeInTheDocument();
  });

  it("never meters sign-in and enables submit on any password", async () => {
    const services = fakeServices();
    services.checkEmail = async () => ({ existing: true, status: "CREDENTIAL" });
    renderCard({ instance: PASSWORD_ONLY }, services);
    submitEmail("ada@example.com");
    await screen.findByTestId("space-auth-password-form");
    expect(screen.queryByLabelText("Confirm your password")).toBeNull();
    const form = screen.getByTestId("space-auth-password-form") as HTMLFormElement;
    expect(form.getAttribute("action")).toBe("/auth/spaces/sign-in/");
    expect(form.querySelector('input[name="confirm_password"]')).toBeNull();
    const submit = screen.getByRole("button", { name: "Sign in" });
    expect(submit).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "abc" } });
    expect(screen.queryByTestId("space-auth-strength")).toBeNull();
    expect(submit).toBeEnabled();
  });

  it("waits for the token before enabling submit", async () => {
    const gate = deferred<string>();
    const services = fakeServices();
    services.readCsrfToken = () => gate.promise;
    await renderPasswordStep({ instance: FULL }, services);
    expect(screen.getByRole("button", { name: "Get started" })).toBeDisabled();
    gate.resolve("csrf-token");
    fireEvent.change(screen.getByLabelText("Choose a password"), { target: { value: "Str0ng!Pass1" } });
    fireEvent.change(screen.getByLabelText("Confirm your password"), { target: { value: "Str0ng!Pass1" } });
    await screen.findByTestId("space-auth-password-form");
    expect(screen.getByRole("button", { name: "Get started" })).toBeEnabled();
  });

  it("shows progress on submit", async () => {
    await renderPasswordStep({});
    fireEvent.change(screen.getByLabelText("Choose a password"), { target: { value: "Str0ng!Pass1" } });
    fireEvent.change(screen.getByLabelText("Confirm your password"), { target: { value: "Str0ng!Pass1" } });
    const submit = screen.getByRole("button", { name: "Get started" });
    expect(submit).toBeEnabled();
    fireEvent.submit(screen.getByTestId("space-auth-password-form"));
    expect(submit).toBeDisabled();
  });

  it("offers the code switch in sign-in with mail on", async () => {
    const services = fakeServices();
    services.checkEmail = async () => ({ existing: true, status: "CREDENTIAL" });
    renderCard({}, services);
    submitEmail("ada@example.com");
    await screen.findByTestId("space-auth-password-form");
    fireEvent.click(screen.getByRole("button", { name: "Use a login code instead" }));
    await screen.findByTestId("space-auth-code-form");
    expect(card()).toHaveAttribute("data-step", "code");
    expect(services.generated).toEqual(["ada@example.com"]);
  });

  it("hides the code switch without mail", async () => {
    const services = fakeServices();
    services.checkEmail = async () => ({ existing: true, status: "CREDENTIAL" });
    renderCard({ instance: PASSWORD_ONLY }, services);
    submitEmail("ada@example.com");
    await screen.findByTestId("space-auth-password-form");
    expect(screen.queryByRole("button", { name: "Use a login code instead" })).toBeNull();
  });

  it("hides the code switch in sign-up", async () => {
    await renderPasswordStep({});
    expect(screen.queryByRole("button", { name: "Use a login code instead" })).toBeNull();
  });

  it("banners a refused switch generate on the code step", async () => {
    const services = fakeServices();
    services.checkEmail = async () => ({ existing: true, status: "CREDENTIAL" });
    services.generateCode = () => Promise.reject(new ApiError({ code: "5100", message: "capped" }));
    renderCard({}, services);
    submitEmail("ada@example.com");
    await screen.findByTestId("space-auth-password-form");
    fireEvent.click(screen.getByRole("button", { name: "Use a login code instead" }));
    await screen.findByTestId("space-auth-banner");
    expect(card()).toHaveAttribute("data-step", "code");
    expect(screen.getByTestId("space-auth-banner")).toHaveTextContent(
      "That code has expired. Send another code and try again."
    );
  });

  it("clears back to the email step and keeps the mode", async () => {
    const services = fakeServices();
    services.checkEmail = async () => ({ existing: true, status: "CREDENTIAL" });
    renderCard({ instance: PASSWORD_ONLY }, services);
    submitEmail("ada@example.com");
    await screen.findByTestId("space-auth-password-form");
    fireEvent.click(screen.getByRole("button", { name: "Clear email" }));
    expect(card()).toHaveAttribute("data-step", "email");
    expect(card()).toHaveAttribute("data-mode", "sign-in");
    expect(emailField()).toHaveValue("");
  });
});

describe("provider buttons", () => {
  const WITH_PROVIDERS: SpaceAuthInstanceCapabilities = {
    ...FULL,
    googleEnabled: true,
    githubEnabled: true,
  };

  it("renders nothing without enabled providers", () => {
    renderCard({});
    expect(screen.queryByTestId("space-auth-providers")).toBeNull();
    expect(screen.queryByTestId("space-auth-providers-divider")).toBeNull();
  });

  it("invites with full labels off the password step", () => {
    renderCard({ instance: WITH_PROVIDERS, nextPath: "/issues/a?board=x" });
    const block = screen.getByTestId("space-auth-providers");
    const google = within(block).getByRole("link", { name: "Create your account with Google" });
    expect(google).toHaveAttribute("href", "/auth/google/?next_path=%2Fissues%2Fa%3Fboard%3Dx");
    expect(google).toHaveAttribute("data-provider", "google");
    expect(within(block).getByRole("link", { name: "Create your account with GitHub" })).toHaveAttribute(
      "href",
      "/auth/github/?next_path=%2Fissues%2Fa%3Fboard%3Dx"
    );
    expect(screen.getByTestId("space-auth-providers-divider")).toHaveTextContent("or");
  });

  it("keeps full labels on the code step", async () => {
    const services = fakeServices();
    services.checkEmail = async () => ({ existing: false, status: "MAGIC_CODE" });
    renderCard({ instance: WITH_PROVIDERS }, services);
    submitEmail("ada@example.com");
    await screen.findByTestId("space-auth-code-form");
    const block = screen.getByTestId("space-auth-providers");
    expect(within(block).getByRole("link", { name: "Create your account with Google" })).toHaveTextContent(
      "Create your account with Google"
    );
  });

  it("compacts beside the password step with accessible names", async () => {
    const services = fakeServices();
    services.checkEmail = async () => ({ existing: true, status: "CREDENTIAL" });
    renderCard({ instance: WITH_PROVIDERS }, services);
    submitEmail("ada@example.com");
    await screen.findByTestId("space-auth-password-form");
    const block = screen.getByTestId("space-auth-providers");
    const google = within(block).getByRole("link", { name: "Continue with Google" });
    expect(google).toHaveTextContent("GO");
    expect(google).toHaveAttribute("href", "/auth/google/");
    expect(within(block).getByRole("link", { name: "Continue with GitHub" })).toHaveTextContent("GH");
  });

  it("omits the return address from provider starts when handed none", () => {
    renderCard({ instance: WITH_PROVIDERS, nextPath: "" });
    const block = screen.getByTestId("space-auth-providers");
    expect(within(block).getByRole("link", { name: "Create your account with Google" })).toHaveAttribute(
      "href",
      "/auth/google/"
    );
  });
});

describe("failure banner and coded links", () => {
  it.each([
    ["5065", "password", "sign-in", "Sign-in failed. Check your password and try again."],
    ["5035", "password", "sign-up", "Sign-up failed. Check your details and try again."],
    ["5090", "code", "sign-in", "That code isn't valid. Check your email and try again."],
    ["5092", "code", "sign-up", "That code isn't valid. Check your email and try again."],
    ["5095", "code", "sign-in", "That code has expired. Send another code and try again."],
    ["5100", "code", "sign-in", "That code has expired. Send another code and try again."],
    ["5102", "code", "sign-up", "That code has expired. Send another code and try again."],
    ["5060", "email", "sign-up", "No account uses this address. Create one to get started."],
    ["5030", "email", "sign-up", "This address is already registered. Sign in with it instead."],
    ["5040", "email", "sign-up", "Enter your email address and password to create an account."],
    ["5020", "password", "sign-up", "That password isn't valid. Try again."],
    ["5115", "email", "sign-up", "Google couldn't sign you in. Try again."],
  ])("lands error %s on the %s step in %s", (code, step, mode, message) => {
    renderCard({ initialErrorCode: code });
    expect(card()).toHaveAttribute("data-step", step);
    expect(card()).toHaveAttribute("data-mode", mode);
    expect(screen.getByTestId("space-auth-banner")).toHaveTextContent(message);
  });

  it.each([["5900"], ["5112"], ["9999"]])("stays silent on %s", (code) => {
    renderCard({ initialErrorCode: code });
    expect(card()).toHaveAttribute("data-step", "email");
    expect(screen.queryByTestId("space-auth-banner")).toBeNull();
  });

  it("accepts numeric codes", () => {
    renderCard({ initialErrorCode: 5065 });
    expect(card()).toHaveAttribute("data-step", "password");
    expect(screen.getByTestId("space-auth-banner")).toHaveTextContent(
      "Sign-in failed. Check your password and try again."
    );
  });

  it("dismisses the banner and keeps the step", () => {
    renderCard({ initialErrorCode: "5065" });
    expect(card()).toHaveAttribute("data-step", "password");
    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByTestId("space-auth-banner")).toBeNull();
    expect(card()).toHaveAttribute("data-step", "password");
  });

  it("consumes the code once", () => {
    const services = fakeServices();
    const { rerender } = render(<SpaceAuthCard instance={FULL} services={services} initialErrorCode="5065" />);
    expect(card()).toHaveAttribute("data-step", "password");
    rerender(<SpaceAuthCard instance={FULL} services={services} initialErrorCode="5092" />);
    expect(card()).toHaveAttribute("data-step", "password");
    expect(screen.getByTestId("space-auth-banner")).toHaveTextContent(
      "Sign-in failed. Check your password and try again."
    );
  });

  it("reads a locked code step without an address", () => {
    renderCard({ initialErrorCode: "5092" });
    expect(screen.getByLabelText("Email address")).toHaveValue("");
    expect(screen.queryByRole("button", { name: "Clear email" })).toBeNull();
  });
});

describe("default services", () => {
  it("checks through the same-origin client when none is injected", async () => {
    const seen: string[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(async (url: string): Promise<Response> => {
        seen.push(url);
        if (url.endsWith("/auth/get-csrf-token/")) {
          return new Response(JSON.stringify({ csrf_token: "tok" }), { status: 200 });
        }
        return new Response(JSON.stringify({ existing: false, status: "CREDENTIAL" }), { status: 200 });
      })
    );
    render(<SpaceAuthCard instance={PASSWORD_ONLY} />);
    submitEmail("ada@example.com");
    await screen.findByTestId("space-auth-password-form");
    expect(seen.some((url) => url.endsWith("/auth/spaces/email-check/"))).toBe(true);
  });
});
