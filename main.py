import asyncio
import logging
import json
from ingestion.pipeline import DataPipeline
from ingestion.sources.websocket import WebSocketSource
from ingestion.sources.polling import PollingSource
from models.state import StateObject

# Configure logging
logging.basicConfig(
    level=logging.INFO,
    format='%(asctime)s - %(name)s - %(levelname)s - %(message)s'
)

async def agent_subscriber(state: StateObject):
    """Simulates an autonomous agent receiving a verified state update."""
    print(f"[{state.timestamp.strftime('%H:%M:%S.%f')[:-3]}] AGENT UPDATE: {state.key} -> {state.value:.2f} (Source: {state.source})")

async def main():
    # 1. Initialize Pipeline
    pipeline = DataPipeline()

    # 2. Add Sources
    # High-frequency WebSocket source for BTC/USD
    pipeline.add_source(WebSocketSource(name="BinanceWS", key="BTC/USD"))
    
    # Low-frequency Polling source for ETH/USD
    pipeline.add_source(PollingSource(name="CoinGeckoAPI", key="ETH/USD", interval=2.0))

    # 3. Subscribe Agent
    pipeline.subscribe(agent_subscriber)

    # 4. Run Pipeline
    print("--- Starting Streaming Data Pipeline ---")
    await pipeline.start()

    try:
        # Run for 10 seconds to demonstrate activity
        await asyncio.sleep(10)
    finally:
        await pipeline.stop()
        print("--- Pipeline Stopped ---")

if __name__ == "__main__":
    asyncio.run(main())
