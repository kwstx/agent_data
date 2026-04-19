import statistics
from datetime import datetime
from typing import List, Dict, Optional
from models.state import RawEvent, StateObject

class DataTransformer:
    """
    Handles normalization, deterministic transformations, and outlier filtering.
    """
    def __init__(self, outlier_threshold: float = 3.0):
        self.outlier_threshold = outlier_threshold
        self._history: Dict[str, List[float]] = {}
        self._max_history = 50

    def normalize(self, event: RawEvent) -> Optional[StateObject]:
        """
        Transforms a RawEvent into a normalized StateObject.
        """
        # 1. Extraction (Source-specific parsing)
        try:
            value = self._extract_value(event)
            if value is None:
                return None
        except Exception:
            return None

        # 2. Deterministic Timestamp Alignment
        # Align to nearest millisecond for sub-second precision
        aligned_ts = event.ingested_at.replace(microsecond=(event.ingested_at.microsecond // 1000) * 1000)

        # 3. Outlier Filtering
        if self._is_outlier(event.key, value):
            return None

        # 4. Source Reconciliation Metadata
        metadata = {
            "ingestion_latency_ms": (datetime.utcnow() - event.ingested_at).total_seconds() * 1000,
            "raw": event.raw_data
        }

        # 5. Return Structured State Object
        return StateObject(
            key=event.key,
            value=value,
            timestamp=aligned_ts,
            source=event.source,
            metadata=metadata
        )

    def _extract_value(self, event: RawEvent) -> Optional[float]:
        """Normalize heterogeneous inputs into a unified float value."""
        raw = event.raw_data
        if isinstance(raw, dict):
            return float(raw.get("price") or raw.get("value") or 0.0)
        elif isinstance(raw, (int, float)):
            return float(raw)
        return None

    def _is_outlier(self, key: str, value: float) -> bool:
        """Simple outlier detection using Z-score or historical deviation."""
        if key not in self._history:
            self._history[key] = []
        
        history = self._history[key]
        if len(history) < 10:
            history.append(value)
            return False

        mean = statistics.mean(history)
        stdev = statistics.stdev(history)

        if stdev == 0:
            return False

        z_score = abs(value - mean) / stdev
        
        # Update history
        history.append(value)
        if len(history) > self._max_history:
            history.pop(0)

        return z_score > self.outlier_threshold
