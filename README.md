# Verifiable Real-Time Data & State Access Layer

## Overview

The Verifiable Real-Time Data & State Access Layer is a decentralized protocol designed to provide autonomous agents with low-latency, cryptographically auditable access to external and on-chain state. The system eliminates the requirement for centralized intermediaries by implementing a three-layer architecture focused on sub-second data freshness, quorum-based consensus, and compact proof generation.

By leveraging Ed25519 signatures and Merkle Tree state commitments, the protocol ensures that any data consumed by an agent can be verified against a distributed set of independent nodes or validated on-chain via a verification anchor.

## System Architecture

The protocol is structured into three primary operational layers:

### 1. Off-Chain Ingestion and Processing Layer
*   **Rust-Based Nodes**: High-performance ingestion engines that connect to external data sources (e.g., blockchain RPCs, exchange WebSockets, public APIs).
*   **Normalization**: Data points are normalized into a standardized state schema, including timestamp alignment and source metadata.
*   **Frequency**: Optimized for sub-500ms updates to maintain state relevance for high-frequency autonomous operations.

### 2. Cryptographically Verifiable Aggregation Layer
*   **Quorum Consensus**: Observations must be signed by a majority (m-of-n) of participating nodes before being accepted as valid state.
*   **Deterministic Aggregation**: Provides resilience against outliers through deterministic functions (e.g., median for numerical data, mode for categorical state).
*   **Merkle Accountability**: Aggregated state is hashed into a Merkle Tree. This enables the generation of compact inclusion proofs, allowing agents to verify specific keys without downloading the entire state cluster.

### 3. Unified State Access Layer
*   **REST API**: Synchronous access for on-demand state queries and Merkle proof retrieval.
*   **WebSocket Streaming**: Asynchronous push-based updates that broadcast verified snapshots to subscribed agents every consensus epoch.
*   **Agent SDK**: A Python-based interface that abstracts the cryptographic verification logic, ensuring that only verified data reaches the agent's decision-making logic.

## Technical Specifications

| Component | Technology | Responsibility |
| :--- | :--- | :--- |
| Core Node | Rust (Axum, Tokio) | Ingestion, Consensus, Merkle Tree Construction |
| State Access | REST / WebSockets | Data Distribution & Proof Delivery |
| SDK | Python (Aiohttp, Ed25519) | Simplified Interaction & Client-Side Verification |
| Security | Ed25519 / Merkle Proofs | Integrity & Authenticity Guarantees |
| Verification | Solidity | On-Chain Anchor for Quorum Validation |

## API Reference

### State Queries
*   `GET /state`: Retrieves the complete verified state snapshot for the current epoch, including signatures from the quorum.
*   `GET /state/:key`: Retrieves a specific state value along with its Merkle inclusion proof.

### Streaming
*   `WS /ws`: Real-time stream of verified state objects. Each message contains the `QuorumSnapshot` which includes the Merkle root and the aggregated values.

### Node Management
*   `POST /nodes/join`: Protocol onboarding endpoint. Requires node identifier and stake commitment for permissionless participation.

### Adversarial Simulation
*   `POST /simulation/chaos`: Adjusts network latency and fault injection parameters to test system resilience.
*   `POST /simulation/load`: Configures high-frequency request generation for performance profiling.

## Developer Setup

### Prerequisites
*   Rust (1.70+ recommended)
*   Python (3.9+)
*   Etherum Development Environment (Foundry or Hardhat for contract interaction)

### Running the VNode
1. Navigate to the `vnode` directory.
2. Build the project:
   ```bash
   cargo build --release
   ```
3. Execute the node:
   ```bash
   ./target/release/vnode
   ```

### Using the Python SDK
Initialize the `VNodeClient` to interact with the protocol from an autonomous agent:

```python
from sdk import VNodeClient

async def main():
    async with VNodeClient(base_url="http://localhost:3000") as client:
        # Fetch verified state with automatic cryptographic check
        price = await client.get_verified_value("ETH/USD")
        print(f"Verified ETH Price: {price}")
```

## Security and Verification Logic

The protocol enforces security through two distinct verification paths:

1.  **Direct Signature Verification**: The SDK verifies that the incoming state snapshot contains valid Ed25519 signatures from a majority of the registered node public keys.
2.  **Merkle Proof Validation**: For individual key-value pairs, the SDK reconstructs the Merkle root using the provided inclusion path and compares it against the signed root from the quorum.
3.  **On-Chain Anchor**: For cross-chain operations, a smart contract validates the quorum's signatures and enforces a block-time freshness constraint (e.g., state must be < 300 seconds old) before the state is accepted by on-chain logic.

## Project Roadmap

*   **Current Phase**: Implementation of decentralized protocol network with permissionless node onboarding and staking.
*   **Upcoming**: Support for generalized JSON schemas to handle complex arbitrary state transitions beyond numerical values.
*   **Future**: Cross-chain state commitments for direct query-verify-act loops across heterogeneous execution environments.
