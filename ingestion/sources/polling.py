import asyncio
import random
import logging
from .base import BaseSource

class PollingSource(BaseSource):
    """
    Ingests data by periodically polling an external source.
    """
    def __init__(self, name: str, key: str, interval: float = 5.0):
        super().__init__(name, key)
        self.interval = interval
        self._task = None

    async def start(self):
        self._running = True
        self._task = asyncio.create_task(self._poll_loop())
        logging.info(f"PollingSource {self.name} started.")

    async def stop(self):
        self._running = False
        if self._task:
            self._task.cancel()
            try:
                await self._task
            except asyncio.CancelledError:
                pass
        logging.info(f"PollingSource {self.name} stopped.")

    async def _poll_loop(self):
        while self._running:
            try:
                # Mock polling request
                data = await self.fetch_data()
                await self.emit(data)
            except Exception as e:
                logging.error(f"Error in polling loop for {self.name}: {e}")
            await asyncio.sleep(self.interval)

    async def fetch_data(self) -> float:
        # Simulations a fetch from an API
        return 100.0 + random.uniform(-1.0, 1.0)
