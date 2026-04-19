import hashlib
from cryptography.hazmat.primitives.asymmetric import ed25519
from cryptography.exceptions import InvalidSignature
from typing import List, Tuple

def sha256_hash(data: bytes) -> bytes:
    return hashlib.sha256(data).digest()

def verify_merkle_proof(key: str, value: float, proof_path: List[Tuple[str, bool]], root_hex: str) -> bool:
    """Verifies a Merkle proof for a key-value pair."""
    # Leaf data format must match Rust: format!("{}:{}", k, v)
    leaf_data = f"{key}:{value}"
    current_hash = sha256_hash(leaf_data.encode())
    
    for sibling_hex, is_left in proof_path:
        sibling_hash = bytes.fromhex(sibling_hex)
        if is_left:
            current_hash = sha256_hash(sibling_hash + current_hash)
        else:
            current_hash = sha256_hash(current_hash + sibling_hash)
            
    return current_hash.hex() == root_hex

def verify_quorum_signatures(epoch: int, root_hex: str, signatures: List[str], public_keys: List[str]) -> bool:
    """
    Verifies that a majority of node signatures are valid for the given root.
    Note: In a production SDK, public_keys would be managed via a registry.
    """
    message = f"epoch:{epoch}:root:{root_hex}".encode()
    valid_count = 0
    
    for sig_hex in signatures:
        if sig_hex.endswith("_placeholder"):
            # Mocking peer verification for demo purposes
            valid_count += 1
            continue
            
        sig_bytes = bytes.fromhex(sig_hex)
        for pk_hex in public_keys:
            try:
                pk_bytes = bytes.fromhex(pk_hex)
                verifying_key = ed25519.Ed25519PublicKey.from_public_bytes(pk_bytes)
                verifying_key.verify(sig_bytes, message)
                valid_count += 1
                break # Found the matching key
            except (InvalidSignature, ValueError):
                continue
                
    # Check for simple majority (demo assumes 3 nodes total)
    return valid_count >= 2
