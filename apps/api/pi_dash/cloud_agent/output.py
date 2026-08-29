from typing import Annotated, Literal

from pydantic import BaseModel, Field, StringConstraints

Evidence = Annotated[str, StringConstraints(strip_whitespace=True, max_length=1000)]
Limitation = Annotated[str, StringConstraints(strip_whitespace=True, max_length=500)]


class CloudAgentOutput(BaseModel):
    outcome: Literal["completed", "blocked", "noop"]
    summary: str = Field(max_length=30_000)
    evidence: list[Evidence] = Field(default_factory=list, max_length=10)
    limitations: list[Limitation] = Field(default_factory=list, max_length=10)
