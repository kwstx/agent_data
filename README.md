# Verifiable Real-Time Data & State Access Layer

This repository contains a full-stack verifiable data pipeline designed for autonomous agents, ensuring sub-second data freshness and cryptographic auditability.

## System Layers

1.  **Off-chain Ingestion (Python/Rust)**: High-frequency data ingestion from multiple sources (WebSockets/RPC).
2.  **Verifiable Aggregation (Rust - `vnode`)**: Quorum-based consensus, deterministic median aggregation, and Merkle tree state construction.
3.  **On-Chain Verification Anchor (Solidity - `contracts`)**: Minimal verification anchors that validate quorum signatures and state freshness before committing state hashes.

## Features
- **Quorum-Based Verifiability**: State is only committed if signed by a majority of independent nodes.
- **Freshness Constraints**: Block-time comparisons prevent the use of stale or manipulated old states.
- **Minimal On-Chain Footprint**: Stores only cryptographic hashes (Merkle roots), keeping execution costs low.
- **Deterministic Reconciliation**: Outlier-resistant functions ensure node agreement on external state.

## Getting Started

### Smart Contracts (Solidity)
- Located in `contracts/StateAnchor.sol`.
- Implements quorum verification and timestamp freshness checks.

### Verifiable Nodes (Rust)
- Located in `vnode/`.
- Handles data ingestion, signing, and local Merkle tree construction.

### Legacy/Mock Pipeline (Python)
- `main.py` provides a high-level orchestration for the ingestion layer.

## Structure
- `contracts/`: Solidity verification anchors.
- `vnode/`: Rust implementation of verifiable aggregation nodes.
- `ingestion/`: Python-based ingestion connectors and processing logic.
- `project_context.md`: Detailed architectural documentation.
