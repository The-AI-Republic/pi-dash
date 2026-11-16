# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for the duplicate-assets endpoint (NEWFRONT-1028).

The duplicate is a server-side S3 copy: the endpoint must build its S3
client against the configured internal endpoint (``AWS_S3_ENDPOINT_URL``)
even when the public host the client reached differs. On parity stacks the
API is served on a dev-server port that is unreachable from inside the API
container, so copying through the stored public host raises
``EndpointConnectionError`` and answers 500.
"""

import os
from unittest.mock import Mock, patch

import pytest
from rest_framework import status

from pi_dash.db.models import FileAsset, Issue, IssueComment


def _duplicate_url(slug, asset_id):
    return f"/api/assets/v2/workspaces/{slug}/duplicate-assets/{asset_id}/"


def _download_url(slug, asset_id):
    return f"/api/assets/v2/workspaces/{slug}/download/{asset_id}/"


_MINIO_ENV = {
    "AWS_ACCESS_KEY_ID": "test-key",
    "AWS_SECRET_ACCESS_KEY": "test-secret",
    "AWS_S3_BUCKET_NAME": "test-bucket",
    "AWS_REGION": "us-east-1",
    "AWS_S3_ENDPOINT_URL": "http://minio:9000",
    "USE_MINIO": "1",
}


@pytest.fixture
def comment(workspace, project, create_user):
    issue = Issue.objects.create(
        name="commented issue",
        project=project,
        workspace=workspace,
        created_by=create_user,
    )
    return IssueComment.objects.create(
        issue=issue,
        project=project,
        workspace=workspace,
        comment_html="<p>with an image</p>",
        comment_stripped="with an image",
        actor=create_user,
    )


@pytest.fixture
def uploaded_asset(workspace, project, create_user, comment):
    return FileAsset.objects.create(
        attributes={"name": "shot.png", "type": "image/png", "size": 12},
        asset="original/key-shot.png",
        size=12,
        workspace=workspace,
        created_by=create_user,
        entity_type=FileAsset.EntityTypeContext.COMMENT_DESCRIPTION,
        project=project,
        comment=comment,
        is_uploaded=True,
    )


@pytest.mark.contract
class TestDuplicateAssetEndpoint:
    @patch.dict(os.environ, _MINIO_ENV, clear=True)
    @patch("pi_dash.settings.storage.boto3")
    @pytest.mark.django_db
    def test_duplicate_copies_through_internal_endpoint(
        self, mock_boto3, session_client, workspace, project, create_user, comment, uploaded_asset
    ):
        """Parity-stack shape: public host differs from the S3 endpoint, still 200."""
        mock_s3 = Mock()
        mock_boto3.client.return_value = mock_s3

        response = session_client.post(
            _duplicate_url(workspace.slug, uploaded_asset.id),
            {
                "entity_id": str(comment.id),
                "entity_type": FileAsset.EntityTypeContext.COMMENT_DESCRIPTION,
                "project_id": str(project.id),
            },
            format="json",
            HTTP_HOST="localhost:3018",
        )

        assert response.status_code == status.HTTP_200_OK
        assert str(response.data["asset_id"])
        duplicated = FileAsset.objects.get(id=response.data["asset_id"])
        assert duplicated.is_uploaded is True

        endpoint = mock_boto3.client.call_args[1]["endpoint_url"]
        assert endpoint == "http://minio:9000"

        _, copy_kwargs = mock_s3.copy_object.call_args
        assert copy_kwargs["Bucket"] == "test-bucket"
        assert copy_kwargs["CopySource"] == {
            "Bucket": "test-bucket",
            "Key": "original/key-shot.png",
        }
        assert isinstance(copy_kwargs["CopySource"]["Key"], str)
        assert copy_kwargs["Key"].startswith(f"{workspace.id}/")
        assert copy_kwargs["Key"].endswith("-shot.png")

    @patch("pi_dash.app.views.asset.v2.S3Storage")
    @pytest.mark.django_db
    def test_duplicate_builds_server_storage_client(
        self, mock_storage_cls, session_client, workspace, project, comment, uploaded_asset
    ):
        """The duplicate path opts into the server (internal-endpoint) client."""
        response = session_client.post(
            _duplicate_url(workspace.slug, uploaded_asset.id),
            {
                "entity_id": str(comment.id),
                "entity_type": FileAsset.EntityTypeContext.COMMENT_DESCRIPTION,
                "project_id": str(project.id),
            },
            format="json",
        )

        assert response.status_code == status.HTTP_200_OK
        assert mock_storage_cls.call_args[1].get("is_server") is True
        source_key, destination_key = mock_storage_cls.return_value.copy_object.call_args[0]
        assert source_key == "original/key-shot.png"
        assert isinstance(source_key, str)
        assert destination_key.startswith(f"{workspace.id}/")

    @patch("pi_dash.app.views.asset.v2.S3Storage")
    @pytest.mark.django_db
    def test_download_still_signs_against_request_host(
        self, mock_storage_cls, session_client, workspace, uploaded_asset
    ):
        """Presigned download flows keep the public host (no behavior change)."""
        mock_storage_cls.return_value.generate_presigned_url.return_value = "http://testserver/signed/asset"

        response = session_client.get(_download_url(workspace.slug, uploaded_asset.id))

        assert response.status_code == status.HTTP_302_FOUND
        assert mock_storage_cls.call_args[1].get("is_server", False) is False
