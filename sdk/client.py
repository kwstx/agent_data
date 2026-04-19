import asyncio
import aiohttp
import websockets
import json
import logging
from typing import Callable, List, Optional, Any
from .models import QuorumSnapshot, QueryResponse
from .crypto import verify_merkle_proof, verify_quorum_signatures

logger = logging.getLogger("VNodeSDK")

class VNodeClient:
    """
    Unified SDK for interacting with the Verifiable State Layer.
    Abstracts connection management, cryptographic verification, and state deserialization.
    """
    def __init__(self, base_url: str = "http://localhost:3000"):
        self.base_url = base_url.rstrip("/")
        self.ws_url = self.base_url.replace("http", "ws") + "/ws"
        self.node_pks: List[str] = []
        self._session: Optional[aiohttp.ClientSession] = None

    async def __aenter__(self):
        await self.connect()
        return self

    async def __aexit__(self, exc_type, exc_val, exc_tb):
        await self.close()

    async def connect(self):
        """Initializes the connection and fetches trusted node public keys."""
        if not self._session:
            self._session = aiohttp.ClientSession()
        
        # Fetch node keys for verification
        async with self._session.get(f"{self.base_url}/nodes") as resp:
            self.node_pks = await resp.json()
            logger.info(f"SDK connected. Discovered {len(self.node_pks)} nodes for quorum verification.")

    async def close(self):
        if self._session:
            await self._session.close()
            self._session = None

    async def get_snapshot(self) -> QuorumSnapshot:
        """
        Fetches the latest global state snapshot.
        Enforces quorum signature verification before returning.
        """
        if not self._session: await self.connect()
        
        async with self._session.get(f"{self.base_url}/state") as resp:
            data = await resp.json()
            snapshot = QuorumSnapshot(**data)
            
            if not verify_quorum_signatures(snapshot.epoch, snapshot.merkle_root, snapshot.signatures, self.node_pks):
                raise SecurityError("Quorum signature verification failed for global snapshot")
            
            return snapshot

    async def get_verified_value(self, key: str) -> float:
        """
        Fetches a single verified value by key.
        Performs both Merkle proof validation and quorum verification.
        """
        if not self._session: await self.connect()
        
        async with self._session.get(f"{self.base_url}/state/{key}") as resp:
            if resp.status == 404:
                raise KeyError(f"Key '{key}' not found in verifiable state")
            
            data = await resp.json()
            res = QueryResponse(**data)
            
            # 1. Verify signatures on the root provided in the proof
            # For brevity in this demo, we assume the signatures in ProofData cover the root.
            if not verify_quorum_signatures(0, res.proof.merkle_root, res.proof.signatures, self.node_pks):
                raise SecurityError(f"Quorum verification failed for {key} proof")
            
            # 2. Verify Merkle Path
            if not verify_merkle_proof(key, res.value, res.proof.proof_path, res.proof.merkle_root):
                raise SecurityError(f"Merkle inclusion proof failed for {key}")
            
            return res.value

    async def subscribe(self, callback: Callable[[QuorumSnapshot], Any]):
        """
        Agent-facing subscription interface.
        Handles WebSocket lifecycle and background verification.
        """
        logger.info(f"Subscribing to verified stream at {self.ws_url}")
        
        async for websocket in websockets.connect(self.ws_url):
            try:
                async for message in websocket:
                    try:
                        data = json.loads(message)
                        snapshot = QuorumSnapshot(**data)
                        
                        # Internal verification
                        if verify_quorum_signatures(snapshot.epoch, snapshot.merkle_root, snapshot.signatures, self.node_pks):
                            await callback(snapshot)
                        else:
                            logger.warning(f"Discarding unverified snapshot (Epoch {snapshot.epoch})")
                    except Exception as e:
                        logger.error(f"Error processing stream update: {e}")
            except websockets.ConnectionClosed:
                logger.warning("WebSocket connection lost. Reconnecting...")
                await asyncio.sleep(1)

class SecurityError(Exception):
    """Raised when cryptographic verification fails."""
    pass
