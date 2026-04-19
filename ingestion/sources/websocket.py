import asyncio
import random
import logging
from .base import BaseSource

class WebSocketSource(BaseSource):
    """
    Ingests data via event-driven WebSocket connections.
    """
    def __init__(self, name: str, key: str):
        super().__init__(name, key)
        self._task = None

    async def start(self):
        self._running = True
        self._task = asyncio.create_task(self._process_stream())
        logging.info(f"WebSocketSource {self.name} started.")

    async def stop(self):
        self._running = False
        if self._task:
            self._task.cancel()
            try:
                await self._task
            except asyncio.CancelledError:
                pass
        logging.info(f"WebSocketSource {self.name} stopped.")

    async def _process_stream(self):
        # In a real scenario, this would manage the websocket connection
        while self._running:
            try:
                # Simulate receiving a message from a websocket
                await asyncio.sleep(random.uniform(0.1, 1.0)) # Sub-second data
                data = {
                    "price": 100.0 + random.uniform(-2.0, 2.0),
                    "v": random.randint(10, 100)
                }
                await self.emit(data)
            except Exception as e:
                logging.error(f"Error in websocket stream for {self.name}: {e}")
