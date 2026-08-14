from rest_framework import status
from rest_framework.exceptions import APIException


class CloudAgentUnavailableAPI(APIException):
    status_code = status.HTTP_409_CONFLICT
    default_detail = {"error": "Pi Dash Cloud Agent is not currently available", "code": "cloud_agent_unavailable"}
    default_code = "cloud_agent_unavailable"
