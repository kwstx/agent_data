import asyncio
import logging
from typing import List, Dict, Callable, Awaitable
from .sources.base import BaseSource
from models.state import RawEvent, StateObject
from processing.transformer import DataTransformer

class DataPipeline:
    """
    Orchestrates the ingestion, transformation, and distribution of state updates.
    """
    def __init__(self):
        self.sources: List[BaseSource] = []
        self.transformer = DataTransformer()
        self.state_cache: Dict[str, StateObject] = {}
        self.subscribers: List[Callable[[StateObject], Awaitable[None]]] = []
        self._queue = asyncio.Queue()
        self._worker_task = None

    def add_source(self, source: BaseSource):
        source.set_callback(self._on_raw_event)
        self.sources.append(source)

    def subscribe(self, callback: Callable[[StateObject], Awaitable[None]]):
        self.subscribers.append(callback)

    async def start(self):
        """Starts the pipeline and all registered sources."""
        self._worker_task = asyncio.create_task(self._process_queue())
        start_tasks = [source.start() for source in self.sources]
        await asyncio.gather(*start_tasks)
        logging.info("Data Pipeline started.")

    async def stop(self):
        """Stops the pipeline and all registered sources."""
        stop_tasks = [source.stop() for source in self.sources]
        await asyncio.gather(*stop_tasks)
        if self._worker_task:
            self._worker_task.cancel()
        logging.info("Data Pipeline stopped.")

    async def _on_raw_event(self, event: RawEvent):
        await self._queue.put(event)

    async def _process_queue(self):
        while True:
            event = await self._queue.get()
            try:
                # 1. Transform & Normalize
                state = self.transformer.normalize(event)
                
                if state:
                    # 2. Update Local Cache (Source Reconciliation happens here)
                    # For now, we simple overwrite with the latest verified state
                    self.state_cache[state.key] = state
                    
                    # 3. Notify Subscribers (Agents or downstream layers)
                    for subscriber in self.subscribers:
                        await subscriber(state)
            except Exception as e:
                logging.error(f"Pipeline error processing event: {e}")
            finally:
                self._queue.task_done()

    def get_latest_state(self, key: str) -> Optional[StateObject]:
        return self.state_cache.get(key)
