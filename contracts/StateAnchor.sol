// SPDX-License-Identifier: MIT
pragma solidity ^0.8.19;

/**
 * @title StateAnchor
 * @dev This contract acts as a minimal on-chain verification anchor for the vnode system.
 * It verifies quorum signatures against registered node public keys and enforces 
 * timestamp freshness constraints before committing the latest state hash.
 */
contract StateAnchor {
    struct StateCommitment {
        bytes32 stateHash;    // The Merkle root or cryptographic representation of global state
        uint256 timestamp;    // The block-time alignment of the state
        uint64 epoch;         // Monotonically increasing sequence number
    }

    // Configurable System Parameters
    uint256 public quorumThreshold;
    uint256 public constant MAX_FRESHNESS_WINDOW = 300; // 5 minutes staleness limit
    
    // Core State
    StateCommitment public latestState;
    mapping(address => bool) public isRegisteredNode;
    uint256 public registeredNodeCount;

    // Events for Auditability
    event StateCommitted(bytes32 indexed stateHash, uint64 indexed epoch, uint256 timestamp);
    event NodeRegistryUpdated(address indexed node, bool isRegistered);
    event QuorumThresholdUpdated(uint256 newThreshold);

    /**
     * @dev Initialize with a minimum signature threshold for state updates.
     */
    constructor(uint256 _initialQuorum) {
        require(_initialQuorum > 0, "Quorum must be positive");
        quorumThreshold = _initialQuorum;
    }

    /**
     * @dev Adds or removes a node from the authorized verification set.
     * In production, this would be governed by a DAO or Multi-sig.
     */
    function setNodeStatus(address node, bool status) external {
        // Implementation of access control omitted for brevity/demo
        if (status && !isRegisteredNode[node]) {
            registeredNodeCount++;
        } else if (!status && isRegisteredNode[node]) {
            registeredNodeCount--;
        }
        isRegisteredNode[node] = status;
        emit NodeRegistryUpdated(node, status);
    }

    /**
     * @dev Updates the quorum requirements for commitment.
     */
    function setQuorumThreshold(uint256 _newThreshold) external {
        require(_newThreshold > 0 && _newThreshold <= registeredNodeCount, "Invalid threshold");
        quorumThreshold = _newThreshold;
        emit QuorumThresholdUpdated(_newThreshold);
    }

    /**
     * @dev Verifies a quorum of signatures and commits the new state hash.
     * @param _stateHash The cryptographic hash to be stored.
     * @param _epoch Sequence marker for the update.
     * @param _timestamp The node-reported timestamp.
     * @param _signatures Sorted array of 65-byte ECDSA signatures (v, r, s).
     */
    function commitState(
        bytes32 _stateHash,
        uint64 _epoch,
        uint256 _timestamp,
        bytes[] calldata _signatures
    ) external {
        // 1. Freshness Constraint Validation
        require(_timestamp <= block.timestamp, "Future timestamp rejected");
        require(block.timestamp - _timestamp <= MAX_FRESHNESS_WINDOW, "State freshness expired");
        
        // 2. Epoch Ordering
        require(_epoch > latestState.epoch, "Non-monotonic epoch detected");

        // 3. Quorum Signature Verification
        require(_signatures.length >= quorumThreshold, "Insufficient signatures for consensus");
        
        // Prepare data for verification
        bytes32 messageHash = keccak256(abi.encodePacked(_stateHash, _epoch, _timestamp));
        bytes32 ethSignedMessageHash = keccak256(abi.encodePacked("\x19Ethereum Signed Message:\n32", messageHash));

        address lastSigner = address(0);
        uint256 validSigs = 0;

        for (uint256 i = 0; i < _signatures.length; i++) {
            address signer = _recoverSigner(ethSignedMessageHash, _signatures[i]);
            
            // Verification constraints:
            // - Signer must be in the authorized set
            // - Signers must be sorted to prevent double-counting/duplicates in O(n)
            require(signer > lastSigner, "Signatures must be unique and sorted");
            require(isRegisteredNode[signer], "Unauthorized node signature");
            
            validSigs++;
            lastSigner = signer;
        }

        require(validSigs >= quorumThreshold, "Post-verification quorum check failed");

        // 4. Update the verification anchor (Minimal storage overhead)
        latestState = StateCommitment({
            stateHash: _stateHash,
            timestamp: _timestamp,
            epoch: _epoch
        });

        emit StateCommitted(_stateHash, _epoch, _timestamp);
    }

    /**
     * @dev Internal helper to recover ECDSA signer from signature bytes.
     */
    function _recoverSigner(bytes32 _hash, bytes memory _signature) internal pure returns (address) {
        if (_signature.length != 65) revert("Invalid signature length");
        bytes32 r;
        bytes32 s;
        uint8 v;
        assembly {
            r := mload(add(_signature, 32))
            s := mload(add(_signature, 64))
            v := byte(0, mload(add(_signature, 96)))
        }
        return ecrecover(_hash, v, r, s);
    }
}
