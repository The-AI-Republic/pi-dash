# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django import forms
from django.contrib import admin

from pi_dash.prompting.models import PromptTemplate


class PromptTemplateForm(forms.ModelForm):
    class Meta:
        model = PromptTemplate
        fields = "__all__"
        widgets = {
            "body": forms.Textarea(attrs={"rows": 40, "cols": 120, "class": "vLargeTextField"}),
        }


@admin.register(PromptTemplate)
class PromptTemplateAdmin(admin.ModelAdmin):
    form = PromptTemplateForm
    list_display = ("name", "workspace", "is_active", "version", "updated_at")
    list_filter = ("name", "is_active", "workspace")
    search_fields = ("name", "workspace__slug")
    readonly_fields = ("id", "created_at", "updated_at", "version")

    def get_readonly_fields(self, request, obj=None):
        ro = list(super().get_readonly_fields(request, obj))
        if obj and obj.is_global_default:
            # Guard the global default against accidental workspace-admin edits;
            # superusers still edit via shell or the management command.
            if not request.user.is_superuser:
                ro.extend(["workspace", "name", "body", "is_active"])
        return ro
