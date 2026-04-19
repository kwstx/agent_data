import asyncio
from abc import ABC, abstractmethod
from typing import Any, Callable, Awaitable
from models.state import RawEvent

class BaseSource(ABC):
    """
    Abstract base class for all data ingestion sources.
    """
    def __init__(self, name: str, key: str):
        self.name = name
        self.key = key
        self.on_event: Optional[Callable[[RawEvent], Awaitable[None]]] = None
        self._running = False

    def set_callback(self, callback: Callable[[RawEvent], Awaitable[None]]):
        self.on_event = callback

    @abstractmethod
    async def start(self):
        """Start the ingestion process."""
        pass

    @abstractmethod
    async def stop(self):
        """Stop the ingestion process."""
        pass

    async def emit(self, raw_data: Any):
        """Emit a raw event to the pipeline."""
        if self.on_event:
            event = RawEvent(
                source=self.name,
                key=self.key,
                raw_data=raw_data
            )
            await self.on_event(event)
