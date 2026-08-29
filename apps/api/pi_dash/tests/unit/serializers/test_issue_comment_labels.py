# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest

from pi_dash.api.serializers.issue import IssueCommentCreateSerializer


@pytest.mark.unit
def test_comment_create_serializer_accepts_fold_label():
    serializer = IssueCommentCreateSerializer(
        data={"comment_html": "<p>No change</p>", "labels": ["fold"]}
    )

    assert serializer.is_valid(), serializer.errors
    assert serializer.validated_data["labels"] == ["fold"]
