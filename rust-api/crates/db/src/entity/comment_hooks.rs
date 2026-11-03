//! `IssueComment.save()` (`db/models/issue.py:598-647`) and
//! `Description.save()` (`db/models/description.py:23-29`) as
//! `ActiveModelBehavior::before_save` hooks (design D4).
//!
//! Each hook recomputes only its row's own `stripped` column — the
//! orchestration (description create + link, changed-subset sync)
//! lives in `domain::comments`, which loads the stored row it needs.
//!
//! The two strippers differ on purpose: the comment uses
//! `html_processor.strip_tags` (`issue.py:21`, entities decoded),
//! the description uses `django.utils.html.strip_tags`
//! (`description.py:6`, entities verbatim).

use sea_orm::{ActiveModelBehavior, ActiveValue, ConnectionTrait, DbErr};

use super::descriptions;
use super::issue_comments;
use crate::app_pages::strip::{django_strip_tags, ml_strip_tags};

/// `self.comment_stripped = strip_tags(self.comment_html) if
/// self.comment_html != "" else ""` (`issue.py:606-610`).
pub fn comment_stripped_for(comment_html: &str) -> String {
    if comment_html.is_empty() {
        String::new()
    } else {
        ml_strip_tags(comment_html)
    }
}

/// `self.description_stripped = None if (self.description_html == "" or
/// self.description_html is None) else strip_tags(self.description_html)`
/// (`description.py:25-29`).
pub fn description_stripped_for(description_html: Option<&str>) -> Option<String> {
    match description_html {
        None => None,
        Some("") => None,
        Some(html) => Some(django_strip_tags(html)),
    }
}

fn active_str(value: &ActiveValue<String>) -> Option<&str> {
    match value {
        ActiveValue::Set(value) | ActiveValue::Unchanged(value) => Some(value),
        ActiveValue::NotSet => None,
    }
}

#[async_trait::async_trait]
impl ActiveModelBehavior for issue_comments::ActiveModel {
    async fn before_save<C>(mut self, _db: &C, _insert: bool) -> Result<Self, DbErr>
    where
        C: ConnectionTrait,
    {
        // `issue.py:606-610`. On update the services `Set` the new HTML
        // (or leave it `Unchanged`, in which case the stored stripped
        // value this recompute would reproduce is already there).
        if let Some(html) = active_str(&self.comment_html) {
            let stripped = comment_stripped_for(html);
            self.comment_stripped = ActiveValue::Set(stripped);
        }
        Ok(self)
    }
}

#[async_trait::async_trait]
impl ActiveModelBehavior for descriptions::ActiveModel {
    async fn before_save<C>(mut self, _db: &C, _insert: bool) -> Result<Self, DbErr>
    where
        C: ConnectionTrait,
    {
        // `description.py:23-29`. Overwrites unconditionally — including
        // the `description_stripped` a caller passed to
        // `Description.objects.create`, which never survives `save()`.
        if let Some(html) = active_str(&self.description_html) {
            let stripped = description_stripped_for(Some(html));
            self.description_stripped = ActiveValue::Set(stripped);
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comment_stripped_empty_stays_empty() {
        assert_eq!(comment_stripped_for(""), "");
        assert_eq!(comment_stripped_for("<p>seed</p>"), "seed");
    }

    #[test]
    fn comment_stripped_decodes_entities() {
        assert_eq!(comment_stripped_for("a &amp; b"), "a & b");
    }

    #[test]
    fn description_stripped_none_when_empty() {
        assert_eq!(description_stripped_for(None), None);
        assert_eq!(description_stripped_for(Some("")), None);
        assert_eq!(
            description_stripped_for(Some("<p>seed</p>")),
            Some("seed".to_owned())
        );
    }

    #[test]
    fn description_stripped_keeps_entities() {
        assert_eq!(
            description_stripped_for(Some("a &amp; b")),
            Some("a &amp; b".to_owned())
        );
    }
}
