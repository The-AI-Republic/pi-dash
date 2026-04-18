/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

export default {
  sidebar: {
    projects: "Projects",
    pages: "Pages",
    new_work_item: "New work item",
    home: "Home",
    your_work: "Your work",
    inbox: "Inbox",
    workspace: "Workspace",
    views: "Views",
    analytics: "Analytics",
    work_items: "Work items",
    cycles: "Cycles",
    modules: "Modules",
    intake: "Intake",
    drafts: "Drafts",
    favorites: "Favorites",
    pro: "Pro",
    upgrade: "Upgrade",
    stickies: "Stickies",
  },

  auth: {
    common: {
      email: {
        label: "Email",
        placeholder: "name@company.com",
        errors: {
          required: "Email is required",
          invalid: "Email is invalid",
        },
      },
      password: {
        label: "Password",
        set_password: "Set a password",
        placeholder: "Enter password",
        confirm_password: {
          label: "Confirm password",
          placeholder: "Confirm password",
        },
        current_password: {
          label: "Current password",
        },
        new_password: {
          label: "New password",
          placeholder: "Enter new password",
        },
        change_password: {
          label: {
            default: "Change password",
            submitting: "Changing password",
          },
        },
        errors: {
          match: "Passwords don't match",
          empty: "Please enter your password",
          length: "Password length should me more than 8 characters",
          strength: {
            weak: "Password is weak",
            strong: "Password is strong",
          },
        },
        submit: "Set password",
        toast: {
          change_password: {
            success: {
              title: "Success!",
              message: "Password changed successfully.",
            },
            error: {
              title: "Error!",
              message: "Something went wrong. Please try again.",
            },
          },
        },
      },
      unique_code: {
        label: "Unique code",
        placeholder: "123456",
        paste_code: "Paste the code sent to your email",
        requesting_new_code: "Requesting new code",
        sending_code: "Sending code",
      },
      already_have_an_account: "Already have an account?",
      login: "Log in",
      create_account: "Create an account",
      new_to_apple_pi_dash: "New to Apple Pi Dash?",
      back_to_sign_in: "Back to sign in",
      resend_in: "Resend in {seconds} seconds",
      sign_in_with_unique_code: "Sign in with unique code",
      forgot_password: "Forgot your password?",
    },
    sign_up: {
      header: {
        label: "Create an account to start managing work with your team.",
        step: {
          email: {
            header: "Sign up",
            sub_header: "",
          },
          password: {
            header: "Sign up",
            sub_header: "Sign up using an email-password combination.",
          },
          unique_code: {
            header: "Sign up",
            sub_header: "Sign up using a unique code sent to the email address above.",
          },
        },
      },
      errors: {
        password: {
          strength: "Try setting-up a strong password to proceed",
        },
      },
    },
    sign_in: {
      header: {
        label: "Log in to start managing work with your team.",
        step: {
          email: {
            header: "Log in or sign up",
            sub_header: "",
          },
          password: {
            header: "Log in or sign up",
            sub_header: "Use your email-password combination to log in.",
          },
          unique_code: {
            header: "Log in or sign up",
            sub_header: "Log in using a unique code sent to the email address above.",
          },
        },
      },
    },
    forgot_password: {
      title: "Reset your password",
      description: "Enter your user account's verified email address and we will send you a password reset link.",
      email_sent: "We sent the reset link to your email address",
      send_reset_link: "Send reset link",
      errors: {
        smtp_not_enabled: "We see that your god hasn't enabled SMTP, we will not be able to send a password reset link",
      },
      toast: {
        success: {
          title: "Email sent",
          message:
            "Check your inbox for a link to reset your password. If it doesn't appear within a few minutes, check your spam folder.",
        },
        error: {
          title: "Error!",
          message: "Something went wrong. Please try again.",
        },
      },
    },
    reset_password: {
      title: "Set new password",
      description: "Secure your account with a strong password",
    },
    set_password: {
      title: "Secure your account",
      description: "Setting password helps you login securely",
    },
    sign_out: {
      toast: {
        error: {
          title: "Error!",
          message: "Failed to sign out. Please try again.",
        },
      },
    },
  },
  runners: {
    title: "Apple Pi Dash Runner",
    page_title: "{workspace} - Runners",
    toast: {
      error_title: "Error!",
    },
    tabs: {
      runners: "Runners",
      runs: "Runs",
      approvals: "Approvals",
    },
    list: {
      add_runner: "Add a runner",
      cap_count: "You have {active} of {max} runners registered.",
      label_placeholder: "optional label (e.g. my-laptop)",
      mint: "Mint registration code",
      minting: "Minting…",
      cap_reached: "Cap reached",
      mint_failed: "Failed to mint token",
      token_warning: "Copy this once — it will not be shown again.",
      token_run_instructions: "Run on your machine:",
      dismiss_token: "I've saved this — hide it",
      connected_runners: "Connected runners",
      empty: "No runners yet. Mint a registration code to connect your first one.",
      columns: {
        name: "Name",
        status: "Status",
        os_arch: "OS / Arch",
        version: "Version",
        last_heartbeat: "Last heartbeat",
      },
      revoke: "Revoke",
      revoke_confirm_title: "Revoke runner?",
      revoke_confirm_body: "The daemon will be forced offline.",
      revoke_failed: "Failed to revoke runner",
      status: {
        online: "online",
        busy: "busy",
        offline: "offline",
        revoked: "revoked",
      },
    },
    runs: {
      columns: {
        started: "Started",
        status: "Status",
        prompt: "Prompt",
      },
      empty: "No runs yet.",
      select_run: "Select a run on the left.",
      cancel: "Cancel run",
      cancel_confirm_title: "Cancel run?",
      cancel_confirm_body: "The runner will stop this run as soon as it gets the signal.",
      cancel_failed: "Failed to cancel run",
      prompt: "Prompt",
      error: "Error",
      done_payload: "Done payload",
      events_count: "Events ({count})",
      event_columns: {
        seq: "seq",
        kind: "kind",
        at: "at",
      },
      status: {
        queued: "queued",
        assigned: "assigned",
        running: "running",
        awaiting_approval: "awaiting approval",
        awaiting_reauth: "awaiting reauth",
        completed: "completed",
        failed: "failed",
        cancelled: "cancelled",
      },
    },
    approvals: {
      empty: "No pending approvals.",
      run_meta: "Run {runId} · requested {at}",
      expires: "expires {at}",
      accept_once: "Accept once",
      accept_for_session: "Accept for session",
      decline: "Decline",
      decision_failed: "Failed to record decision",
      kinds: {
        command_execution: "The runner wants to run a shell command",
        file_change: "The runner wants to modify a file",
        network_access: "The runner wants to make a network call",
        other: "The runner is requesting approval",
      },
    },
  },
} as const;
