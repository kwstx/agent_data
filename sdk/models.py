from pydantic import BaseModel
from typing import List, Dict, Optional
from datetime import datetime

class ProofData(BaseModel):
    merkle_root: str
    proof_bytes: str
    index: int
    total_leaves: int
    signatures: List[str]

class QueryResponse(BaseModel):
    key: str
    value: f64
    proof: ProofData

class QuorumSnapshot(BaseModel):
    epoch: int
    merkle_root: str
    values: Dict[str, float]
    signatures: List[str]
