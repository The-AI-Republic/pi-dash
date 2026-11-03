#![forbid(unsafe_code)]

//! D-23 doc-meta vocabulary: `SPECTACULAR_SETTINGS` consts + API-key auth scheme (stage 5).
//!
//! Port of `apps/api/pi_dash/settings/openapi.py:11-279` (`SPECTACULAR_SETTINGS`,
//! every key) and `apps/api/pi_dash/utils/openapi/auth.py:15-34`
//! (`APIKeyAuthenticationExtension`). Pure constants; the utoipa doc builder
//! (PIDASHCONV-532) renders from these.
//!
//! Fixture: `rust-api/fixtures/v1_openapi/FX-OPENAPI-01.meta.json`
//! (`FX-OPENAPI-01`); every const below is asserted against it in the tests,
//! tag names, descriptions and order included. The auth scheme has no fixture
//! row (it is not part of `SPECTACULAR_SETTINGS`); its test pins the
//! `auth.py:29-34` literals instead.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

/// Rendered `openapi` version string. Provenance: live-doc capture
/// (drf-spectacular 0.28.0 renders 3.0.3 for every document); see the
/// `openapi_version` key of `FX-OPENAPI-01`.
pub const OPENAPI_VERSION: &str = "3.0.3";

/// `SPECTACULAR_SETTINGS["TITLE"]` (`openapi.py:15`).
pub const TITLE: &str = "The Pi Dash REST API";

/// `SPECTACULAR_SETTINGS["DESCRIPTION"]` (`openapi.py:16-20`).
/// Pieces mirror the Python implicit concatenation.
pub const DESCRIPTION: &str = concat!(
    "The Pi Dash REST API\n\n",
    "Visit our quick start guide and full API documentation at ",
    "[github.com/The-AI-Republic/pi-dash](https://github.com/The-AI-Republic/pi-dash#readme).",
);

/// `SPECTACULAR_SETTINGS["VERSION"]` (`openapi.py:26`).
pub const VERSION: &str = "0.0.1";

/// `SPECTACULAR_SETTINGS["CONTACT"]` (`openapi.py:21-25`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Contact {
    pub name: &'static str,
    pub url: &'static str,
    pub email: &'static str,
}

/// `SPECTACULAR_SETTINGS["CONTACT"]` (`openapi.py:21-25`).
pub const CONTACT: Contact = Contact {
    name: "Pi Dash",
    url: "https://airepublic.com",
    email: "support@airepublic.com",
};

/// `SPECTACULAR_SETTINGS["LICENSE"]` (`openapi.py:27-30`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct License {
    pub name: &'static str,
    pub url: &'static str,
}

/// `SPECTACULAR_SETTINGS["LICENSE"]` (`openapi.py:27-30`).
pub const LICENSE: License = License {
    name: "GNU AGPLv3",
    url: "https://github.com/The-AI-Republic/pi-dash/blob/preview/LICENSE.txt",
};

/// `SPECTACULAR_SETTINGS["SERVE_INCLUDE_SCHEMA"]` (`openapi.py:34`).
pub const SERVE_INCLUDE_SCHEMA: bool = false;

/// `SPECTACULAR_SETTINGS["SCHEMA_PATH_PREFIX"]` (`openapi.py:35`).
pub const SCHEMA_PATH_PREFIX: &str = "/api/v1/";

/// `SPECTACULAR_SETTINGS["SCHEMA_CACHE_TIMEOUT"]` (`openapi.py:36`; 0 disables caching).
pub const SCHEMA_CACHE_TIMEOUT: u32 = 0;

/// `SPECTACULAR_SETTINGS["PREPROCESSING_HOOKS"]` (`openapi.py:40-42`).
/// Django dotted paths, kept verbatim; the Rust port lives in [`crate::v1_openapi::hooks`].
pub const PREPROCESSING_HOOKS: [&str; 1] =
    ["pi_dash.utils.openapi.hooks.preprocess_filter_api_v1_paths"];

/// `SPECTACULAR_SETTINGS["POSTPROCESSING_HOOKS"]` (`openapi.py:43-45`).
/// Django dotted paths, kept verbatim; the Rust port lives in [`crate::v1_openapi::hooks`].
pub const POSTPROCESSING_HOOKS: [&str; 1] =
    ["pi_dash.utils.openapi.hooks.postprocess_project_id_dual_form"];

/// One `SPECTACULAR_SETTINGS["SERVERS"]` entry (`openapi.py:49-52`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Server {
    pub url: &'static str,
    pub description: &'static str,
}

/// `SPECTACULAR_SETTINGS["SERVERS"]` (`openapi.py:49-52`), in order.
pub const SERVERS: [Server; 2] = [
    Server {
        url: "http://localhost:8000",
        description: "Local",
    },
    Server {
        url: "https://airepublic.com/api",
        description: "Production",
    },
];

/// One `SPECTACULAR_SETTINGS["TAGS"]` entry (`openapi.py:56-263`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tag {
    pub name: &'static str,
    pub description: &'static str,
}

/// `SPECTACULAR_SETTINGS["TAGS"]` (`openapi.py:56-263`): all 14 tags in order.
/// Description pieces mirror the Python implicit concatenation per tag.
pub const TAGS: [Tag; 14] = [
    Tag {
        name: "Assets",
        description: concat!(
            "**File Upload & Presigned URLs**\n\n",
            "Generate presigned URLs for direct file uploads to cloud storage. Handle user avatars, ",
            "cover images, and generic project assets with secure upload workflows.\n\n",
            "*Key Features:*\n",
            "- Generate presigned URLs for S3 uploads\n",
            "- Support for user avatars and cover images\n",
            "- Generic asset upload for projects\n",
            "- File validation and size limits\n\n",
            "*Use Cases:* User profile images, project file uploads, secure direct-to-cloud uploads.",
        ),
    },
    Tag {
        name: "Cycles",
        description: concat!(
            "**Sprint & Development Cycles**\n\n",
            "Create and manage development cycles (sprints) to organize work into time-boxed iterations. ",
            "Track progress, assign work items, and monitor team velocity.\n\n",
            "*Key Features:*\n",
            "- Create and configure development cycles\n",
            "- Assign work items to cycles\n",
            "- Track cycle progress and completion\n",
            "- Generate cycle analytics and reports\n\n",
            "*Use Cases:* Sprint planning, iterative development, progress tracking, team velocity.",
        ),
    },
    Tag {
        name: "Intake",
        description: concat!(
            "**Work Item Intake Queue**\n\n",
            "Manage incoming work items through a dedicated intake queue for triage and review. ",
            "Submit, update, and process work items before they enter the main project workflow.\n\n",
            "*Key Features:*\n",
            "- Submit work items to intake queue\n",
            "- Review and triage incoming work items\n",
            "- Update intake work item status and properties\n",
            "- Accept, reject, or modify work items before approval\n\n",
            "*Use Cases:* Work item triage, external submissions, quality review, approval workflows.",
        ),
    },
    Tag {
        name: "Labels",
        description: concat!(
            "**Labels & Tags**\n\n",
            "Create and manage labels to categorize and organize work items. Use color-coded labels ",
            "for easy identification, filtering, and project organization.\n\n",
            "*Key Features:*\n",
            "- Create custom labels with colors and descriptions\n",
            "- Apply labels to work items for categorization\n",
            "- Filter and search by labels\n",
            "- Organize labels across projects\n\n",
            "*Use Cases:* Priority marking, feature categorization, bug classification, team organization.",
        ),
    },
    Tag {
        name: "Members",
        description: concat!(
            "**Team Member Management**\n\n",
            "Manage team members, roles, and permissions within projects and workspaces. ",
            "Control access levels and track member participation.\n\n",
            "*Key Features:*\n",
            "- Invite and manage team members\n",
            "- Assign roles and permissions\n",
            "- Control project and workspace access\n",
            "- Track member activity and participation\n\n",
            "*Use Cases:* Team setup, access control, role management, collaboration.",
        ),
    },
    Tag {
        name: "Modules",
        description: concat!(
            "**Feature Modules**\n\n",
            "Group related work items into modules for better organization and tracking. ",
            "Plan features, track progress, and manage deliverables at a higher level.\n\n",
            "*Key Features:*\n",
            "- Create and organize feature modules\n",
            "- Group work items by module\n",
            "- Track module progress and completion\n",
            "- Manage module leads and assignments\n\n",
            "*Use Cases:* Feature planning, release organization, progress tracking, team coordination.",
        ),
    },
    Tag {
        name: "Projects",
        description: concat!(
            "**Project Management**\n\n",
            "Create and manage projects to organize your development work. Configure project settings, ",
            "manage team access, and control project visibility.\n\n",
            "*Key Features:*\n",
            "- Create, update, and delete projects\n",
            "- Configure project settings and preferences\n",
            "- Manage team access and permissions\n",
            "- Control project visibility and sharing\n\n",
            "*Use Cases:* Project setup, team collaboration, access control, project configuration.",
        ),
    },
    Tag {
        name: "States",
        description: concat!(
            "**Workflow States**\n\n",
            "Define custom workflow states for work items to match your team's process. ",
            "Configure state transitions and track work item progress through different stages.\n\n",
            "*Key Features:*\n",
            "- Create custom workflow states\n",
            "- Configure state transitions and rules\n",
            "- Track work item progress through states\n",
            "- Set state-based permissions and automation\n\n",
            "*Use Cases:* Custom workflows, status tracking, process automation, progress monitoring.",
        ),
    },
    Tag {
        name: "Users",
        description: concat!(
            "**Current User Information**\n\n",
            "Get information about the currently authenticated user including profile details ",
            "and account settings.\n\n",
            "*Key Features:*\n",
            "- Retrieve current user profile\n",
            "- Access user account information\n",
            "- View user preferences and settings\n",
            "- Get authentication context\n\n",
            "*Use Cases:* Profile display, user context, account information, authentication status.",
        ),
    },
    Tag {
        name: "Work Item Activity",
        description: concat!(
            "**Activity History & Search**\n\n",
            "View activity history and search for work items across the workspace. ",
            "Get detailed activity logs and find work items using text search.\n\n",
            "*Key Features:*\n",
            "- View work item activity history\n",
            "- Search work items across workspace\n",
            "- Track changes and modifications\n",
            "- Filter search results by project\n\n",
            "*Use Cases:* Activity tracking, work item discovery, change history, workspace search.",
        ),
    },
    Tag {
        name: "Work Item Attachments",
        description: concat!(
            "**Work Item File Attachments**\n\n",
            "Generate presigned URLs for uploading files directly to specific work items. ",
            "Upload and manage attachments associated with work items.\n\n",
            "*Key Features:*\n",
            "- Generate presigned URLs for work item attachments\n",
            "- Upload files directly to work items\n",
            "- Retrieve and manage attachment metadata\n",
            "- Delete attachments from work items\n\n",
            "*Use Cases:* Screenshots, error logs, design files, supporting documents.",
        ),
    },
    Tag {
        name: "Work Item Comments",
        description: concat!(
            "**Comments & Discussions**\n\n",
            "Add comments and discussions to work items for team collaboration. ",
            "Support threaded conversations, mentions, and rich text formatting.\n\n",
            "*Key Features:*\n",
            "- Add comments to work items\n",
            "- Thread conversations and replies\n",
            "- Mention users and trigger notifications\n",
            "- Rich text and markdown support\n\n",
            "*Use Cases:* Team discussions, progress updates, code reviews, decision tracking.",
        ),
    },
    Tag {
        name: "Work Item Links",
        description: concat!(
            "**External Links & References**\n\n",
            "Link work items to external resources like documentation, repositories, or design files. ",
            "Maintain connections between work items and external systems.\n\n",
            "*Key Features:*\n",
            "- Add external URL links to work items\n",
            "- Validate and preview linked resources\n",
            "- Organize links by type and category\n",
            "- Track link usage and access\n\n",
            "*Use Cases:* Documentation links, repository connections, design references, external tools.",
        ),
    },
    Tag {
        name: "Work Items",
        description: concat!(
            "**Work Items & Tasks**\n\n",
            "Create and manage work items like tasks, bugs, features, and user stories. ",
            "The core entities for tracking work in your projects.\n\n",
            "*Key Features:*\n",
            "- Create, update, and manage work items\n",
            "- Assign to team members and set priorities\n",
            "- Track progress through workflow states\n",
            "- Set due dates, estimates, and relationships\n\n",
            "*Use Cases:* Bug tracking, task management, feature development, sprint planning.",
        ),
    },
];

/// `SPECTACULAR_SETTINGS["AUTHENTICATION_WHITELIST"]` (`openapi.py:267-269`).
pub const AUTHENTICATION_WHITELIST: [&str; 1] =
    ["pi_dash.api.middleware.api_authentication.APIKeyAuthentication"];

/// `SPECTACULAR_SETTINGS["COMPONENT_NO_READ_ONLY_REQUIRED"]` (`openapi.py:273`).
pub const COMPONENT_NO_READ_ONLY_REQUIRED: bool = true;

/// `SPECTACULAR_SETTINGS["COMPONENT_SPLIT_REQUEST"]` (`openapi.py:274`).
pub const COMPONENT_SPLIT_REQUEST: bool = true;

/// `SPECTACULAR_SETTINGS["ENUM_NAME_OVERRIDES"]` (`openapi.py:275-278`),
/// `(enum name, Django model path)` pairs in file order.
pub const ENUM_NAME_OVERRIDES: [(&str, &str); 2] = [
    ("ModuleStatusEnum", "pi_dash.db.models.module.ModuleStatus"),
    (
        "IntakeWorkItemStatusEnum",
        "pi_dash.db.models.intake.IntakeIssueStatus",
    ),
];

/// `APIKeyAuthenticationExtension.target_class` (`auth.py:21`).
pub const API_KEY_AUTH_TARGET_CLASS: &str =
    "pi_dash.api.middleware.api_authentication.APIKeyAuthentication";

/// `APIKeyAuthenticationExtension.name` (`auth.py:22`).
pub const API_KEY_AUTH_NAME: &str = "ApiKeyAuthentication";

/// `APIKeyAuthenticationExtension.priority` (`auth.py:23`).
pub const API_KEY_AUTH_PRIORITY: i32 = 1;

/// `get_security_definition()["type"]` (`auth.py:30`).
pub const API_KEY_SCHEME_TYPE: &str = "apiKey";

/// `get_security_definition()["in"]` (`auth.py:31`).
pub const API_KEY_SCHEME_IN: &str = "header";

/// `get_security_definition()["name"]` (`auth.py:32`).
pub const API_KEY_SCHEME_NAME: &str = "X-API-Key";

/// `get_security_definition()["description"]` (`auth.py:33`), verbatim.
pub const API_KEY_SCHEME_DESCRIPTION: &str =
    "API key authentication. Provide your API key in the X-API-Key header.";

/// Port of `APIKeyAuthenticationExtension.get_security_definition` (`auth.py:25-34`).
/// The `auto_schema` argument is unused by the implementation, so it is dropped.
pub fn api_key_security_definition() -> serde_json::Value {
    serde_json::json!({
        "type": API_KEY_SCHEME_TYPE,
        "in": API_KEY_SCHEME_IN,
        "name": API_KEY_SCHEME_NAME,
        "description": API_KEY_SCHEME_DESCRIPTION,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const FIXTURE: &str = include_str!("../../../../fixtures/v1_openapi/FX-OPENAPI-01.meta.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    #[test]
    fn openapi_version_info_contact_license() {
        let fx = fixture();
        assert_eq!(fx["openapi_version"].as_str().unwrap(), OPENAPI_VERSION);
        let settings = &fx["spectacular_settings"];
        assert_eq!(settings["TITLE"].as_str().unwrap(), TITLE);
        assert_eq!(settings["DESCRIPTION"].as_str().unwrap(), DESCRIPTION);
        assert_eq!(settings["VERSION"].as_str().unwrap(), VERSION);
        assert_eq!(settings["CONTACT"]["name"].as_str().unwrap(), CONTACT.name);
        assert_eq!(settings["CONTACT"]["url"].as_str().unwrap(), CONTACT.url);
        assert_eq!(
            settings["CONTACT"]["email"].as_str().unwrap(),
            CONTACT.email
        );
        assert_eq!(settings["LICENSE"]["name"].as_str().unwrap(), LICENSE.name);
        assert_eq!(settings["LICENSE"]["url"].as_str().unwrap(), LICENSE.url);
    }

    #[test]
    fn schema_flags_and_hook_paths() {
        let settings = &fixture()["spectacular_settings"];
        assert_eq!(
            settings["SERVE_INCLUDE_SCHEMA"].as_bool().unwrap(),
            SERVE_INCLUDE_SCHEMA
        );
        assert_eq!(
            settings["SCHEMA_PATH_PREFIX"].as_str().unwrap(),
            SCHEMA_PATH_PREFIX
        );
        assert_eq!(
            settings["SCHEMA_CACHE_TIMEOUT"].as_u64().unwrap(),
            u64::from(SCHEMA_CACHE_TIMEOUT)
        );
        assert_eq!(
            settings["PREPROCESSING_HOOKS"][0].as_str().unwrap(),
            PREPROCESSING_HOOKS[0]
        );
        assert_eq!(
            settings["POSTPROCESSING_HOOKS"][0].as_str().unwrap(),
            POSTPROCESSING_HOOKS[0]
        );
    }

    #[test]
    fn servers_verbatim_in_order() {
        let servers = fixture()["spectacular_settings"]["SERVERS"].clone();
        let servers = servers.as_array().unwrap();
        assert_eq!(servers.len(), SERVERS.len());
        for (got, want) in SERVERS.iter().zip(servers.iter()) {
            assert_eq!(want["url"].as_str().unwrap(), got.url);
            assert_eq!(want["description"].as_str().unwrap(), got.description);
        }
    }

    #[test]
    fn all_14_tags_verbatim_in_order() {
        let fx = fixture();
        let tags = fx["spectacular_settings"]["TAGS"]
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(tags.len(), 14);
        assert_eq!(tags.len(), TAGS.len());
        for (got, want) in TAGS.iter().zip(tags.iter()) {
            assert_eq!(want["name"].as_str().unwrap(), got.name);
            assert_eq!(want["description"].as_str().unwrap(), got.description);
        }
        let names = fx["pinned"]["tag_names_in_order"]
            .as_array()
            .unwrap()
            .clone();
        let got_names: Vec<&str> = TAGS.iter().map(|tag| tag.name).collect();
        let want_names: Vec<&str> = names.iter().map(|name| name.as_str().unwrap()).collect();
        assert_eq!(got_names, want_names);
    }

    #[test]
    fn auth_whitelist_component_flags_enum_overrides() {
        let settings = &fixture()["spectacular_settings"];
        assert_eq!(
            settings["AUTHENTICATION_WHITELIST"][0].as_str().unwrap(),
            AUTHENTICATION_WHITELIST[0]
        );
        assert_eq!(
            settings["COMPONENT_NO_READ_ONLY_REQUIRED"]
                .as_bool()
                .unwrap(),
            COMPONENT_NO_READ_ONLY_REQUIRED
        );
        assert_eq!(
            settings["COMPONENT_SPLIT_REQUEST"].as_bool().unwrap(),
            COMPONENT_SPLIT_REQUEST
        );
        let overrides = settings["ENUM_NAME_OVERRIDES"].as_object().unwrap();
        assert_eq!(overrides.len(), ENUM_NAME_OVERRIDES.len());
        for (name, path) in ENUM_NAME_OVERRIDES {
            assert_eq!(overrides[name].as_str().unwrap(), path);
        }
    }

    #[test]
    fn api_key_scheme_verbatim() {
        // No FX-OPENAPI-01 row (auth.py is not SPECTACULAR_SETTINGS):
        // pin the auth.py:21-23 + :29-34 literals.
        assert_eq!(
            API_KEY_AUTH_TARGET_CLASS,
            "pi_dash.api.middleware.api_authentication.APIKeyAuthentication"
        );
        assert_eq!(API_KEY_AUTH_NAME, "ApiKeyAuthentication");
        assert_eq!(API_KEY_AUTH_PRIORITY, 1);
        assert_eq!(
            api_key_security_definition(),
            serde_json::json!({
                "type": "apiKey",
                "in": "header",
                "name": "X-API-Key",
                "description": "API key authentication. Provide your API key in the X-API-Key header.",
            })
        );
    }
}
