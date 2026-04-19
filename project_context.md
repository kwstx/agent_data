# Project: Verifiable Real-Time Data & State Access Layer for Autonomous Agents

## Overview
We are building a Verifiable Real-Time Data & State Access Layer for Autonomous Agents.

## Core Principles (Phase 1)
- **Minimal, Production-Grade Core**: Focus on sub-second data freshness, cryptographic verifiability, and a unified agent interface.
- **Hybrid Push-Pull Architecture**: 
    - **Push**: Continuous streaming of high-frequency data (e.g., asset prices, on-chain events) via heartbeat or deviation triggers.
    - **Pull**: On-demand queries returning the latest verified snapshots.
- **Primary Goal**: Establish a fast, reliable, and verifiable data pipeline to prevent agent hallucinations and stale state.

## System Architecture

The system is constructed as a real-time data pipeline composed of three tightly coupled layers:

1.  **Off-chain Ingestion & Processing Layer**: Handles continuous streaming of external and on-chain data into independent nodes. Assets and events are normalized in near real-time.
2.  **Cryptographically Verifiable Aggregation Layer**: Nodes sign and synchronize state, ensuring that every update is verifiable and consistent across the network.
3.  **Unified State Access Layer**: Exposed to autonomous agents, providing a seamless interface for both push-based event streams and pull-based state queries.

### Core Architecture Principles
- **Stream Convergence**: Data flows are implemented using event-driven streams over WebSockets. Updates are triggered by either fixed time intervals (heartbeats) or deviation thresholds.
- **Node-Local State Cache**: Each node maintains a local state cache representing the most recent snapshot of all tracked assets. This ensures that both push and pull requests reference the exact same internal state representation.
- **Independent Synchronization**: Nodes operate independently to normalize and sign data, synchronizing state to maintain a high-integrity, distributed ledger of real-time events.

### Data Flow Overview
```mermaid
graph TD
    subgraph Ingestion ["Off-chain Ingestion & Processing"]
        Ext[External APIs] --> Stream[WebSocket Streams]
        Chain[On-chain Events] --> Stream
    end

    subgraph Aggregation ["Verifiable Aggregation Layer"]
        Stream --> Node1[Independent Node A]
        Stream --> Node2[Independent Node B]
        Node1 --> Sign[Cryptographic Signing]
        Node2 --> Sign
        Sign --> Sync[State Synchronization]
    end

    subgraph Access ["Unified State Access Layer"]
        Sync --> Cache[Local State Cache]
        Cache --> Push[Push: WebSocket Events]
        Cache --> Pull[Pull: Verified Snapshots]
        Push --> Agents[Autonomous Agents]
        Pull --> Agents
    end
```

