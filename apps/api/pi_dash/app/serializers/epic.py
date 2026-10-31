# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Epic read serializer (NEWFRONT-984, ISS-232 cloud leg).

There is no Epic model: an epic is an issue whose type carries
``is_epic``. Epic reads are the issue reads plus the marker the edition
code branches on; writes reuse the issue serializers with the type
forced (see ``views/issue/epics.py``).
"""

# Third Party imports
from rest_framework import serializers

# Module imports
from .issue import IssueSerializer


class EpicIssueSerializer(IssueSerializer):
    is_epic = serializers.SerializerMethodField()

    class Meta(IssueSerializer.Meta):
        # is_epic already rides the parent fields (read from the type);
        # the override below only skips that lookup on epic reads.
        fields = IssueSerializer.Meta.fields

    def get_is_epic(self, obj):
        return True
