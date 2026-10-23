# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

# Django imports
from django.conf import settings
from django.core.validators import MinValueValidator
from django.db import models

# Module imports
from .project import ProjectBaseModel


class IssueWorklog(ProjectBaseModel):
    """One logged time entry on a work item (ISS-206, cloud leg).

    Durations are whole minutes (never fractional hours); ``logged_on``
    is the calendar day the work happened, which may differ from the day
    it was recorded. Entries surface in the activity feed through
    ``worklog.activity.*`` rows and in the sidebar as a per-issue total.
    """

    issue = models.ForeignKey("db.Issue", on_delete=models.CASCADE, related_name="issue_worklogs")
    actor = models.ForeignKey(
        settings.AUTH_USER_MODEL,
        on_delete=models.CASCADE,
        related_name="worklogs",
        null=True,
    )
    minutes = models.PositiveIntegerField(
        validators=[MinValueValidator(1)],
        verbose_name="Logged minutes",
    )
    logged_on = models.DateField(verbose_name="Day worked")
    note = models.TextField(blank=True, default="")

    class Meta:
        db_table = "issue_worklogs"
        ordering = ("-created_at",)

    def __str__(self):
        return f"{self.minutes}m on {self.logged_on} ({self.issue_id})"
