from pydantic import BaseModel, Field
from typing import Any, Dict, Optional
from datetime import datetime

class StateObject(BaseModel):
    """
    The atomic unit of state in the pipeline.
    Represents a normalized data point from any source.
    """
    key: str = Field(..., description="The unique identifier for the state (e.g., 'BTC/USD')")
    value: float = Field(..., description="The numerical value of the state")
    timestamp: datetime = Field(..., description="Deterministic UTC timestamp of the update")
    source: str = Field(..., description="The origin of the data (e.g., 'Binance', 'Chainlink')")
    metadata: Dict[str, Any] = Field(default_factory=dict, description="Additional context from the source")
    
    class Config:
        json_encoders = {
            datetime: lambda v: v.isoformat()
        }

class RawEvent(BaseModel):
    """
    Container for raw data before normalization.
    """
    source: str
    key: str
    raw_data: Any
    ingested_at: datetime = Field(default_factory=datetime.utcnow)
