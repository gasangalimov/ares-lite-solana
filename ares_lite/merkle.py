"""SHA-256 claim Merkle tree, bit-compatible with solana/program.

leaf = SHA256(0x00 || CLAIM_LEAF_TAG || season_id:u64le || claimant:32 || amount:u64le)
node = SHA256(0x01 || CLAIM_NODE_TAG || min(a, b) || max(a, b))

Distinct leaf/node prefixes prevent a 64-byte interior node from being
presented as a leaf (second-preimage style forgery). Sorted pairs make proofs
position-free; an unpaired node is promoted unchanged. One leaf per claimant
per season: amounts are aggregated before the tree is built.
"""

from __future__ import annotations

import hashlib

from .domains import CLAIM_LEAF_TAG, CLAIM_NODE_TAG

U64_MAX = (1 << 64) - 1


def leaf_hash(season_id: int, claimant: bytes, amount: int) -> bytes:
    if len(claimant) != 32:
        raise ValueError("claimant must be a 32-byte public key")
    if not 0 <= season_id <= U64_MAX or not 0 < amount <= U64_MAX:
        raise ValueError("season_id and amount must be u64 (amount > 0)")
    return hashlib.sha256(
        b"\x00" + CLAIM_LEAF_TAG + season_id.to_bytes(8, "little") + claimant + amount.to_bytes(8, "little")
    ).digest()


def node_hash(left: bytes, right: bytes) -> bytes:
    low, high = (left, right) if left <= right else (right, left)
    return hashlib.sha256(b"\x01" + CLAIM_NODE_TAG + low + high).digest()


def build(leaves: list[bytes]) -> tuple[bytes, list[list[bytes]]]:
    """Return (root, proofs) for leaves in the given order."""

    if not leaves:
        raise ValueError("a reward commitment needs at least one leaf")
    level = list(leaves)
    positions = list(range(len(leaves)))
    proofs: list[list[bytes]] = [[] for _ in leaves]
    while len(level) > 1:
        nxt = []
        for index in range(0, len(level), 2):
            if index + 1 < len(level):
                nxt.append(node_hash(level[index], level[index + 1]))
            else:
                nxt.append(level[index])
        for leaf_index, position in enumerate(positions):
            sibling = position ^ 1
            if sibling < len(level):
                proofs[leaf_index].append(level[sibling])
            positions[leaf_index] = position // 2
        level = nxt
    return level[0], proofs


def verify(root: bytes, leaf: bytes, proof: list[bytes]) -> bool:
    node = leaf
    for sibling in proof:
        node = node_hash(node, sibling)
    return node == root
