"""CE provider seam; hosted deployments may overlay admission/model routing."""

from pi_dash.cloud_agent.model import resolve_cloud_model


def resolve_model_for_run(run):
    return resolve_cloud_model()
