/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
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
    prompts: "Prompts",
    schedulers: "Schedulers",
    runners: "AI Agents",
    tooltips: {
      projects: "Browse projects",
      runners: "Manage your AI Agent connectivities",
      prompts: "AI Prompt Templates",
      schedulers: "Recurring AI Agent jobs scoped to projects",
    },
  },

  prompts: {
    title: "Prompts",
    subtitle:
      "System prompt templates that get rendered against each issue before an agent run. Workspace admins can customize the default for this workspace.",
    customize: "Customize for this workspace",
    columns: {
      name: "Name",
      scope: "Scope",
      version: "Version",
      updated: "Updated",
    },
    scope: {
      default: "Pi Dash default",
      workspace: "Workspace override",
    },
    actions: {
      view: "View",
      edit: "Edit",
      revert: "Revert to default",
    },
    list: {
      empty: "No prompt templates available. The Pi Dash default will be seeded on the next migrate.",
    },
    detail: {
      default_title: "Prompt template (Pi Dash default)",
      workspace_title: "Prompt template (workspace override)",
      default_description:
        "This is the built-in Pi Dash default. Workspace admins cannot edit it here — customize for your workspace to override.",
      workspace_description:
        "Your workspace's override of the Pi Dash default. Edits bump the version and apply to the next agent run for this workspace.",
      body: "Template body (Jinja + Markdown)",
      save: "Save",
      unsaved: "Unsaved changes",
      back: "Back to list",
      loading: "Loading…",
      not_found: "Template not found.",
    },
    preview: {
      title: "Preview",
      issue_id_placeholder: "Issue id (UUID)",
      run: "Preview",
      empty: "Paste an issue id and click Preview to render the template against a real issue.",
      failed: "Render failed.",
      missing_issue_id: "Enter an issue id first.",
      admin_only:
        "Previewing the rendered prompt is a workspace-admin action. Ask your workspace admin if you need to see it rendered against a specific issue.",
    },
    revert: {
      confirm_title: "Revert to the Pi Dash default?",
      confirm_body:
        "This archives your workspace-scoped template. New agent runs in this workspace will use the Pi Dash default until you create another override.",
      confirm: "Revert",
    },
    toast: {
      created_title: "Workspace override created",
      created_message: "We copied the current Pi Dash default. Edit and save to customize it.",
      saved_title: "Prompt saved",
      saved_message: "Subsequent agent runs will use the updated prompt.",
      reverted_title: "Reverted to Pi Dash default",
      reverted_message: "This workspace is back on the shared default template.",
      error_title: "Something went wrong",
      customize_failed: "Could not create the workspace override.",
      save_failed: "Could not save the prompt.",
      revert_failed: "Could not revert the prompt.",
    },
  },

  schedulers: {
    title: "Schedulers",
    subtitle:
      "Reusable scheduler definitions for this workspace. Install one on a project to run its prompt against the project on a cron.",
    new: "New scheduler",
    columns: {
      name: "Name",
      slug: "Slug",
      source: "Source",
      installs: "Installs",
      status: "Status",
      updated: "Updated",
    },
    source: {
      builtin: "Built-in",
      manifest: "Manifest",
    },
    status: {
      enabled: "Enabled",
      disabled: "Disabled",
    },
    actions: {
      edit: "Edit",
      delete: "Delete",
    },
    list: {
      empty: "No schedulers in this workspace yet. Click “New scheduler” to create one.",
      installs_count: "{count, plural, one {# install} other {# installs}}",
    },
    form: {
      create_title: "New scheduler",
      edit_title: "Edit scheduler",
      slug_label: "Slug",
      slug_help: "Lowercase identifier used in URLs. Cannot be changed after creation.",
      slug_placeholder: "security-audit",
      name_label: "Name",
      name_placeholder: "Security audit",
      description_label: "Description",
      description_placeholder: "Short summary shown in the install picker.",
      prompt_label: "Prompt",
      prompt_help:
        "The base prompt the agent runs each tick. Per-project context is appended at install time, so keep this prompt project-agnostic.",
      prompt_placeholder: "Look for outstanding security issues in this project…",
      enabled_label: "Enabled",
      enabled_help: "Disabled schedulers cannot be installed on new projects, and existing bindings will not fire.",
      cancel: "Cancel",
      save: "Save",
      create: "Create scheduler",
      saving: "Saving…",
      creating: "Creating…",
      errors: {
        slug_required: "Slug is required.",
        name_required: "Name is required.",
        prompt_required: "Prompt is required.",
      },
    },
    delete: {
      confirm_title: "Delete scheduler?",
      confirm_body:
        "This soft-deletes the scheduler. Any active project bindings will stop firing. The slug becomes available for re-creation.",
      confirm: "Delete",
    },
    toast: {
      created_title: "Scheduler created",
      created_message: "Project admins can now install it on their projects.",
      updated_title: "Scheduler updated",
      updated_message: "Subsequent runs will use the updated definition.",
      deleted_title: "Scheduler deleted",
      deleted_message: "Active bindings have stopped firing.",
      error_title: "Something went wrong",
      create_failed: "Could not create the scheduler.",
      update_failed: "Could not update the scheduler.",
      delete_failed: "Could not delete the scheduler.",
    },
  },

  scheduler_bindings: {
    tab_label: "Schedulers",
    title: "Schedulers",
    subtitle:
      "Schedulers installed on this project. Each install fires its prompt against the project on the configured cron.",
    install: "Install scheduler",
    columns: {
      name: "Scheduler",
      cron: "Schedule",
      next_run: "Next run",
      last_run: "Last run",
      status: "Status",
      updated: "Updated",
    },
    status: {
      enabled: "Enabled",
      disabled: "Disabled",
    },
    actions: {
      edit: "Edit",
      uninstall: "Uninstall",
      enable: "Enable scheduler",
      disable: "Disable scheduler",
    },
    list: {
      empty:
        "No schedulers installed on this project yet. Click “Install scheduler” to add one from the workspace catalog.",
      none_yet: "(never)",
    },
    install_modal: {
      title: "Install scheduler",
      scheduler_label: "Scheduler",
      scheduler_help: "Pick from your workspace's enabled schedulers. Already-installed ones aren't listed.",
      none_available_title: "No schedulers available",
      none_available_body:
        "Either every workspace scheduler is already installed on this project, or your workspace admin hasn't enabled any. Visit Workspace → Schedulers to manage the catalog.",
      cron_label: "Schedule (cron)",
      cron_help: "5-field cron expression in UTC, e.g. ``0 9 * * *`` for 09:00 UTC every day.",
      cron_placeholder: "0 9 * * *",
      extra_context_label: "Project context (optional)",
      extra_context_help:
        "Appended to the scheduler's base prompt at run time. Use it to give project-specific framing the workspace prompt shouldn't carry.",
      extra_context_placeholder: "Notes specific to this project…",
      enabled_label: "Enabled",
      enabled_help: "Disabled installs do not fire on the cron until re-enabled.",
      install: "Install",
      installing: "Installing…",
      cancel: "Cancel",
      errors: {
        scheduler_required: "Pick a scheduler.",
        cron_required: "Cron expression is required.",
      },
    },
    edit_modal: {
      title: "Edit scheduler install",
      save: "Save",
      saving: "Saving…",
    },
    uninstall_modal: {
      title: "Uninstall scheduler?",
      body: "The scheduler stops firing on this project. The workspace definition is unaffected and can be re-installed later.",
      confirm: "Uninstall",
    },
    toast: {
      installed_title: "Scheduler installed",
      installed_message: "It will fire on the configured cron.",
      updated_title: "Install updated",
      updated_message: "Subsequent runs use the new settings.",
      enabled_message: "Scheduler enabled — it will fire on the next cron tick.",
      disabled_message: "Scheduler disabled — it will not fire until re-enabled.",
      uninstalled_title: "Scheduler uninstalled",
      uninstalled_message: "It will not fire on this project until reinstalled.",
      error_title: "Something went wrong",
      install_failed: "Could not install the scheduler.",
      update_failed: "Could not update the install.",
      uninstall_failed: "Could not uninstall the scheduler.",
    },
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
      new_to_pi_dash: "New to Pi Dash?",
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
    title: "AI Agents",
    page_title: "{workspace} - AI Agents",
    toast: {
      error_title: "Error!",
    },
    tabs: {
      runners: "AI Agents",
      runs: "Runs",
      approvals: "Approvals",
    },
    list: {
      add_runner: "Add runner",
      how_it_works_title: "How to add a runner",
      how_it_works_body:
        '1. Click "Add runner", pick a project + pod and submit. The cloud mints a one-time enrollment token bound to that runner.\n2. On the machine that will host the runner, run the displayed `pidash connect --url ... --token ... --host-label ...` command.\n3. The daemon enrolls and the runner shows online here.\n\nEach runner has its own token. The first runner enrolled on a host also bootstraps a machine token used by the `pidash` CLI for non-runner commands.\n\nPrerequisite: the agent CLI (codex / claude) must already be installed on the host.',
      project_placeholder: "Select a project",
      copied: "Copied!",
      copy_failed: "Could not copy to clipboard",
      connected_runners: "Runners",
      empty: 'No runners yet. Click "Add runner" to mint your first per-runner enrollment token.',
      columns: {
        name: "Name",
        status: "Status",
        os_arch: "OS / Arch",
        version: "Version",
        last_heartbeat: "Last heartbeat",
      },
      delete: "Delete",
      delete_confirm_title: "Delete runner?",
      delete_confirm_body:
        "The runner row is removed and the daemon is forced offline. Historic runs are preserved with a null runner reference.",
      delete_failed: "Failed to delete runner",
      status: {
        online: "online",
        busy: "busy",
        offline: "offline",
        revoked: "revoked",
      },
      columns_pod: "Pod",
      columns_connection: "Connection",
    },
    connections: {
      title: "Connections",
      help: "A connection pairs one dev machine with this workspace. Each can host multiple runners.",
      add: "Add connection",
      adding: "Creating…",
      name_placeholder: "optional name (defaults to connection_NNN)",
      empty: "No connections yet. Click Add connection to pair your first dev machine.",
      create_failed: "Failed to create connection",
      token_warning: "Copy this once — the enrollment token will not be shown again.",
      token_run_instructions: "Run this on the machine that will host the runner:",
      copy_command: "Copy command",
      next_step_runner: "Then add a runner under this connection:",
      copy_runner_command: "Copy runner command",
      dismiss_token: "I've saved this — hide it",
      status: {
        pending: "pending enrollment",
        active: "active",
      },
      columns: {
        name: "Name",
        host: "Host",
        status: "Status",
        runner_count: "Runners",
        last_seen: "Last seen",
      },
      delete: "Delete",
      delete_confirm_title: "Delete connection?",
      delete_confirm_body:
        "The connection and every runner under it will be removed. Historic runs are preserved with a null runner reference.",
      delete_failed: "Failed to delete connection",
    },
    pods: {
      title: "Pods",
      help: "Pods group your runners. Issues delegate to a pod, and any free runner inside picks up the work.",
      empty: "No pods yet — your workspace pod will appear here.",
      default_badge: "default",
      runner_count: "{count} runner(s)",
      load_failed: "Failed to load pods",
    },
    add_modal: {
      title: "Add runner",
      subtitle:
        "Mint a one-time enrollment token for a new runner. You'll run the displayed `pidash connect` command on the machine that will host it.",
      project_label: "Project",
      project_help: "The project this runner will work on.",
      pod_label: "Pod (optional)",
      pod_help: "Defaults to the project's default pod.",
      pod_default_option: "(default pod)",
      name_label: "Name (optional)",
      name_help: "Auto-assigned if blank, e.g. ``runner_001``.",
      name_placeholder: "my-laptop-runner",
      host_label_label: "Host label (optional)",
      host_label_help:
        "Free-form host name baked into the suggested command. The daemon will substitute its actual hostname if you leave the flag off.",
      host_label_placeholder: "my-laptop",
      working_dir_label: "Working directory (optional)",
      working_dir_help:
        "Local path the daemon runs the agent CLI in — usually the project repo on disk. Defaults to a sandbox under the runner's data dir, which is rarely what you want.",
      working_dir_placeholder: "local dev machine project working dir",
      submit: "Mint enrollment token",
      submitting: "Minting…",
      cancel: "Cancel",
      done: "Done",
      token_warning: "Copy this once — the enrollment token will not be shown again.",
      token_instructions: "Run this on the machine that will host the runner:",
      copy_command: "Copy command",
      copied: "Copied!",
      runner_id_label: "Runner id",
      errors: {
        project_required: "Pick a project.",
        load_projects_failed: "Could not load projects.",
        load_pods_failed: "Could not load pods.",
        create_failed: "Could not mint the enrollment token.",
      },
    },
    machine_token_note: {
      title: "Machine tokens",
      body: "The first time a runner enrolls on a new host (i.e., a new ``host_label``), the cloud also issues a machine token used by the ``pidash`` CLI for non-runner commands (issue, comment, state). Subsequent runners on the same host reuse that token.",
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
