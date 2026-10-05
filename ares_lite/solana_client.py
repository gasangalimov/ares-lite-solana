"""Minimal Solana client for ARES Lite (devnet / local validator only).

Dependency-free beyond the repository's pinned `httpx` and `pycryptodome`:
legacy transaction encoding, Ed25519 signing, PDA derivation and the few RPC
calls Lite needs. Keys are read from Solana-CLI JSON keypair files that live
OUTSIDE the repository (default ~/.config/ares-lite/); this module never
writes a key into the working tree.
"""

from __future__ import annotations

import base64
import hashlib
import json
import os
import time
from dataclasses import dataclass
from pathlib import Path

ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
SYSTEM_PROGRAM = bytes(32)
DEVNET_URL = "https://api.devnet.solana.com"
LOCAL_URL = "http://127.0.0.1:8899"
MAINNET_MARKERS = ("mainnet", "api.mainnet-beta.solana.com")
# Only release/release.sh sets this, after an interactive confirmation.
MAINNET_GO_PHRASE = "OWNER-GO-MAINNET-IRREVERSIBLE"


def b58encode(data: bytes) -> str:
    number = int.from_bytes(data, "big")
    out = ""
    while number:
        number, rem = divmod(number, 58)
        out = ALPHABET[rem] + out
    return "1" * (len(data) - len(data.lstrip(b"\0"))) + out


def b58decode(text: str) -> bytes:
    number = 0
    for char in text:
        number = number * 58 + ALPHABET.index(char)
    body = number.to_bytes((number.bit_length() + 7) // 8, "big") if number else b""
    return b"\0" * (len(text) - len(text.lstrip("1"))) + body


TOKEN_PROGRAM = b58decode("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
ATA_PROGRAM = b58decode("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL")

# ---------------------------------------------------------------- ed25519

_P = 2**255 - 19
_D = (-121665 * pow(121666, _P - 2, _P)) % _P


def is_on_curve(point: bytes) -> bool:
    """RFC 8032 point decompression succeeds (PDAs must be OFF the curve)."""

    y = int.from_bytes(point, "little") & ((1 << 255) - 1)
    sign = point[31] >> 7
    if y >= _P:
        return False
    u = (y * y - 1) % _P
    v = (_D * y * y + 1) % _P
    x = (u * pow(v, 3, _P) * pow(u * pow(v, 7, _P), (_P - 5) // 8, _P)) % _P
    vxx = (v * x * x) % _P
    if vxx == u % _P:
        pass
    elif vxx == (-u) % _P:
        x = (x * pow(2, (_P - 1) // 4, _P)) % _P
    else:
        return False
    return not (x == 0 and sign == 1)


def create_program_address(seeds: list[bytes], program_id: bytes) -> bytes:
    digest = hashlib.sha256(b"".join(seeds) + program_id + b"ProgramDerivedAddress").digest()
    if is_on_curve(digest):
        raise ValueError("address is on the curve")
    return digest


def find_program_address(seeds: list[bytes], program_id: bytes) -> tuple[bytes, int]:
    for bump in range(255, -1, -1):
        try:
            return create_program_address(seeds + [bytes([bump])], program_id), bump
        except ValueError:
            continue
    raise ValueError("no viable bump")


class Keypair:
    def __init__(self, seed: bytes) -> None:
        from Crypto.PublicKey import ECC

        if len(seed) != 32:
            raise ValueError("Ed25519 seed must be 32 bytes")
        self._key = ECC.construct(curve="Ed25519", seed=seed)
        self.seed = seed
        self.public = self._key.public_key().export_key(format="raw")

    @classmethod
    def generate(cls) -> "Keypair":
        import os

        return cls(os.urandom(32))

    @classmethod
    def load(cls, path: Path) -> "Keypair":
        raw = bytes(json.loads(Path(path).read_text()))
        keypair = cls(raw[:32])
        if keypair.public != raw[32:]:
            raise ValueError("keypair file public key mismatch")
        return keypair

    def save(self, path: Path) -> None:
        path = Path(path)
        repo = Path(__file__).resolve().parents[1]
        if repo in path.resolve().parents:
            raise ValueError("refusing to write a private key inside the repository")
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(list(self.seed + self.public)))
        path.chmod(0o600)

    def sign(self, message: bytes) -> bytes:
        from Crypto.Signature import eddsa

        return eddsa.new(self._key, "rfc8032").sign(message)

    @property
    def address(self) -> str:
        return b58encode(self.public)


# ---------------------------------------------------------- transactions


@dataclass(frozen=True)
class Meta:
    key: bytes
    signer: bool
    writable: bool


@dataclass(frozen=True)
class Ix:
    program: bytes
    accounts: tuple[Meta, ...]
    data: bytes


def _shortvec(n: int) -> bytes:
    out = bytearray()
    while True:
        byte = n & 0x7F
        n >>= 7
        if n:
            out.append(byte | 0x80)
        else:
            out.append(byte)
            return bytes(out)


def compile_message(payer: bytes, instructions: list[Ix], blockhash: bytes) -> tuple[bytes, list[bytes]]:
    """Legacy message; returns (message bytes, ordered signer keys)."""

    flags: dict[bytes, list[bool]] = {payer: [True, True]}
    order = [payer]
    for ix in instructions:
        for meta in ix.accounts:
            if meta.key not in flags:
                flags[meta.key] = [False, False]
                order.append(meta.key)
            flags[meta.key][0] |= meta.signer
            flags[meta.key][1] |= meta.writable
        if ix.program not in flags:
            flags[ix.program] = [False, False]
            order.append(ix.program)

    def group(signer: bool, writable: bool) -> list[bytes]:
        return [k for k in order if flags[k] == [signer, writable] and k != payer]

    keys = [payer] + group(True, True) + group(True, False) + group(False, True) + group(False, False)
    signers = [payer] + group(True, True) + group(True, False)
    readonly_signed = len(group(True, False))
    readonly_unsigned = len(group(False, False))
    out = bytearray([len(signers), readonly_signed, readonly_unsigned])
    out += _shortvec(len(keys)) + b"".join(keys) + blockhash + _shortvec(len(instructions))
    for ix in instructions:
        out.append(keys.index(ix.program))
        out += _shortvec(len(ix.accounts)) + bytes(keys.index(m.key) for m in ix.accounts)
        out += _shortvec(len(ix.data)) + ix.data
    return bytes(out), signers


def sign_transaction(payer: Keypair, signers: list[Keypair], instructions: list[Ix], blockhash: bytes) -> bytes:
    message, order = compile_message(payer.public, instructions, blockhash)
    by_key = {k.public: k for k in [payer, *signers]}
    signatures = [by_key[key].sign(message) for key in order]
    return _shortvec(len(signatures)) + b"".join(signatures) + message


# ------------------------------------------------------------------ RPC


class Rpc:
    def __init__(self, url: str) -> None:
        if any(marker in url for marker in MAINNET_MARKERS) and os.environ.get("ARES_LITE_MAINNET_GO") != MAINNET_GO_PHRASE:
            raise ValueError("ARES Lite tooling refuses mainnet endpoints (separate owner GO decision required)")
        import httpx

        self.url = url
        self._client = httpx.Client(timeout=30)
        self._id = 0

    def call(self, method: str, params: list | None = None):
        self._id += 1
        response = self._client.post(self.url, json={"jsonrpc": "2.0", "id": self._id, "method": method, "params": params or []})
        response.raise_for_status()
        body = response.json()
        if "error" in body:
            raise RuntimeError(f"{method}: {body['error']}")
        return body["result"]

    def blockhash(self) -> bytes:
        return b58decode(self.call("getLatestBlockhash", [{"commitment": "confirmed"}])["value"]["blockhash"])

    def rent(self, size: int) -> int:
        return self.call("getMinimumBalanceForRentExemption", [size])

    def balance(self, key: bytes) -> int:
        return self.call("getBalance", [b58encode(key), {"commitment": "confirmed"}])["value"]

    def sweep(self, wallet: Keypair, destination: bytes) -> int:
        """Return a temporary wallet's remaining SOL (minus the fee) to `destination`."""

        amount = self.balance(wallet.public) - LAMPORTS_PER_SIGNATURE
        if amount <= 0:
            return 0
        self.send(wallet, [system_transfer_ix(wallet.public, destination, amount)])
        return amount

    def slot(self) -> int:
        return self.call("getSlot", [{"commitment": "confirmed"}])

    def account(self, key: bytes) -> dict | None:
        value = self.call("getAccountInfo", [b58encode(key), {"encoding": "base64", "commitment": "confirmed"}])["value"]
        if value is None:
            return None
        value["raw"] = base64.b64decode(value["data"][0])
        return value

    def airdrop(self, key: bytes, lamports: int) -> None:
        """Test SOL for a key. If ARES_LITE_FUNDER names a funded keypair file
        (devnet: the public faucet is rate-limited), transfer from it instead."""

        funder = os.environ.get("ARES_LITE_FUNDER")
        if funder:
            payer = Keypair.load(Path(funder))
            if payer.public != key:
                self.send(payer, [system_transfer_ix(payer.public, key, lamports)])
                return
        self.confirm(self.call("requestAirdrop", [b58encode(key), lamports]))

    def simulate(self, payer: Keypair, instructions: list[Ix], signers: list[Keypair] = ()) -> dict:
        """Simulate without committing (used for non-destructive release checks)."""

        tx = sign_transaction(payer, list(signers), instructions, self.blockhash())
        return self.call("simulateTransaction", [base64.b64encode(tx).decode(),
                                                 {"encoding": "base64", "commitment": "confirmed", "sigVerify": True}])["value"]

    def simulate_unsigned(self, payer: bytes, instructions: list[Ix], watch: list[bytes] = ()) -> dict:
        """Simulate as `payer` WITHOUT any private key (sigVerify off, zero
        signatures). Commits nothing; returns err, logs and the post-state of
        `watch` accounts. Used for release checks on keys the operator lacks
        (e.g. the founder)."""

        message, order = compile_message(payer, instructions, self.blockhash())
        tx = _shortvec(len(order)) + bytes(64 * len(order)) + message
        config = {"encoding": "base64", "commitment": "confirmed", "sigVerify": False, "replaceRecentBlockhash": True}
        if watch:
            config["accounts"] = {"encoding": "base64", "addresses": [b58encode(a) for a in watch]}
        return self.call("simulateTransaction", [base64.b64encode(tx).decode(), config])["value"]

    def send(self, payer: Keypair, instructions: list[Ix], signers: list[Keypair] = ()) -> str:
        tx = sign_transaction(payer, list(signers), instructions, self.blockhash())
        signature = self.call("sendTransaction", [base64.b64encode(tx).decode(), {"encoding": "base64", "preflightCommitment": "confirmed"}])
        self.confirm(signature)
        return signature

    def confirm(self, signature: str, timeout: float = 60) -> None:
        deadline = time.time() + timeout
        while time.time() < deadline:
            status = self.call("getSignatureStatuses", [[signature]])["value"][0]
            if status and status.get("err"):
                raise RuntimeError(f"transaction failed: {status['err']}")
            if status and status.get("confirmationStatus") in ("confirmed", "finalized"):
                return
            time.sleep(0.5)
        raise TimeoutError(signature)

    def beacon(self, slot: int) -> tuple[int, bytes]:
        """Blockhash of the first confirmed block at or after `slot`."""

        blocks = self.call("getBlocks", [slot, slot + 64, {"commitment": "confirmed"}])
        if not blocks:
            raise RuntimeError("beacon slot not yet produced")
        block = self.call("getBlock", [blocks[0], {"transactionDetails": "none", "rewards": False, "commitment": "confirmed"}])
        return blocks[0], b58decode(block["blockhash"])


# --------------------------------------------------------- ARES Lite ixs


def u64(n: int) -> bytes:
    return n.to_bytes(8, "little")


class LiteProgram:
    """Instruction builders for solana/program v2 (fixed supply)."""

    def __init__(self, program_id: bytes, mint: bytes) -> None:
        self.program_id = program_id
        self.mint = mint
        self.config = find_program_address([b"config", mint], program_id)[0]
        self.vault = find_program_address([b"vault", mint], program_id)[0]
        self.vault_authority = find_program_address([b"vault_authority", mint], program_id)[0]

    def season(self, season_id: int) -> bytes:
        return find_program_address([b"season", self.config, u64(season_id)], self.program_id)[0]

    def receipt(self, season: bytes, claimant: bytes) -> bytes:
        return find_program_address([b"claim", season, claimant], self.program_id)[0]

    def bounty(self, bounty_id: int) -> bytes:
        return find_program_address([b"bounty", self.config, u64(bounty_id)], self.program_id)[0]

    def escrow(self, bounty_id: int) -> bytes:
        return find_program_address([b"escrow", self.bounty(bounty_id)], self.program_id)[0]

    def initialize(self, deployer: bytes, founder_token_account: bytes, claim_delay_seconds: int) -> Ix:
        return Ix(self.program_id, (
            Meta(deployer, True, True), Meta(self.config, False, True), Meta(self.mint, False, True),
            Meta(self.vault, False, True), Meta(self.vault_authority, False, False),
            Meta(founder_token_account, False, True), Meta(SYSTEM_PROGRAM, False, False), Meta(TOKEN_PROGRAM, False, False),
        ), b"\x00" + u64(claim_delay_seconds))

    def create_season(self, admin: bytes, season_id: int, manifest_hash: bytes) -> Ix:
        return Ix(self.program_id, (
            Meta(admin, True, True), Meta(self.config, False, False), Meta(self.season(season_id), False, True),
            Meta(SYSTEM_PROGRAM, False, False),
        ), b"\x01" + u64(season_id) + manifest_hash)

    def close_season(self, admin: bytes, season_id: int, log_head: bytes, close_slot: int) -> Ix:
        return Ix(self.program_id, (
            Meta(admin, True, False), Meta(self.config, False, True), Meta(self.season(season_id), False, True),
        ), b"\x07" + log_head + u64(close_slot))

    def publish(self, admin: bytes, season_id: int, root: bytes, total: int, log_head: bytes, results_digest: bytes) -> Ix:
        return Ix(self.program_id, (
            Meta(admin, True, False), Meta(self.config, False, True), Meta(self.season(season_id), False, True),
        ), b"\x02" + root + u64(total) + log_head + results_digest)

    def cancel_root(self, admin: bytes, season_id: int) -> Ix:
        return Ix(self.program_id, (
            Meta(admin, True, False), Meta(self.config, False, True), Meta(self.season(season_id), False, True),
        ), b"\x08")

    def claim(self, claimant: bytes, season_id: int, amount: int, proof: list[bytes], destination: bytes) -> Ix:
        season = self.season(season_id)
        return Ix(self.program_id, (
            Meta(claimant, True, True), Meta(self.config, False, True), Meta(season, False, True),
            Meta(self.receipt(season, claimant), False, True), Meta(self.vault, False, True),
            Meta(destination, False, True), Meta(self.vault_authority, False, False),
            Meta(TOKEN_PROGRAM, False, False), Meta(SYSTEM_PROGRAM, False, False),
        ), b"\x03" + u64(amount) + bytes([len(proof)]) + b"".join(proof))

    def set_admin(self, admin: bytes, new_admin: bytes) -> Ix:
        return Ix(self.program_id, (Meta(admin, True, False), Meta(self.config, False, True)), b"\x04" + new_admin)

    def fund_bounty(self, funder: bytes, funder_token_account: bytes, bounty_id: int, amount: int) -> Ix:
        return Ix(self.program_id, (
            Meta(funder, True, True), Meta(funder_token_account, False, True), Meta(self.config, False, True),
            Meta(self.mint, False, True), Meta(self.bounty(bounty_id), False, True), Meta(self.escrow(bounty_id), False, True),
            Meta(SYSTEM_PROGRAM, False, False), Meta(TOKEN_PROGRAM, False, False),
        ), b"\x05" + u64(bounty_id) + u64(amount))

    def award_bounty(self, admin: bytes, bounty_id: int, destination: bytes) -> Ix:
        return Ix(self.program_id, (
            Meta(admin, True, False), Meta(self.config, False, True), Meta(self.bounty(bounty_id), False, True),
            Meta(self.escrow(bounty_id), False, True), Meta(destination, False, True), Meta(TOKEN_PROGRAM, False, False),
        ), b"\x06")


def parse_config(raw: bytes) -> dict:
    """Decode the v2 Config account (layout mirrors solana/program)."""

    if len(raw) != 197 or raw[0] != 11:
        raise ValueError("not an ARES Lite v3 config account")
    num = lambda at: int.from_bytes(raw[at:at + 8], "little")  # noqa: E731
    genesis = num(130)
    return {
        "version": raw[1], "admin": b58encode(raw[2:34]), "mint": b58encode(raw[34:66]),
        "vault": b58encode(raw[66:98]), "founder": b58encode(raw[98:130]),
        "genesis_ts": genesis - (1 << 64) if genesis >= 1 << 63 else genesis,
        "committed": num(138), "distributed": num(146), "bounty_burned": num(154),
        "bounty_escrowed": num(162), "bounties_awarded": num(170),
        "last_settlement_ts": num(181), "claim_delay": num(189),
    }


def parse_season(raw: bytes) -> dict:
    if len(raw) != 187 or raw[0] != 12:
        raise ValueError("not an ARES Lite v3 season account")
    num = lambda at: int.from_bytes(raw[at:at + 8], "little")  # noqa: E731
    return {"status": ("OPEN", "COMMITTED", "CLOSED")[raw[1]], "season_id": num(2), "manifest_hash": raw[10:42].hex(),
            "close_slot": num(42), "reward_root": raw[50:82].hex(), "reward_total": num(82), "log_head": raw[90:122].hex(),
            "claimed": num(122), "results_digest": raw[131:163].hex(), "claim_open_ts": num(163),
            "prev_settlement_ts": num(171), "close_cap": num(179)}


def parse_mint(raw: bytes) -> dict:
    def opt(chunk: bytes):
        return b58encode(chunk[4:36]) if int.from_bytes(chunk[0:4], "little") == 1 else None

    return {"mint_authority": opt(raw[0:36]), "supply": int.from_bytes(raw[36:44], "little"),
            "decimals": raw[44], "initialized": raw[45] == 1, "freeze_authority": opt(raw[46:82])}


def mint_to_ix(mint: bytes, destination: bytes, authority: bytes, amount: int) -> Ix:
    return Ix(TOKEN_PROGRAM, (Meta(mint, False, True), Meta(destination, False, True), Meta(authority, True, False)),
              bytes([7]) + u64(amount))


LAMPORTS_PER_SIGNATURE = 5_000  # Solana base fee; rehearsal txs set no priority fee
TOKEN_ACCOUNT_LEN = 165
CLAIM_RECEIPT_LEN = 50
BOUNTY_LEN = 59


def close_token_account_ix(account: bytes, destination: bytes, owner: bytes) -> Ix:
    """SPL CloseAccount (balance must be 0): returns the rent to `destination`."""
    return Ix(TOKEN_PROGRAM, (Meta(account, False, True), Meta(destination, False, True), Meta(owner, True, False)), b"\x09")


def system_transfer_ix(source: bytes, destination: bytes, lamports: int) -> Ix:
    return Ix(SYSTEM_PROGRAM, (Meta(source, True, True), Meta(destination, False, True)),
              (2).to_bytes(4, "little") + lamports.to_bytes(8, "little"))


def create_account_ix(payer: bytes, new: bytes, lamports: int, space: int, owner: bytes) -> Ix:
    data = (0).to_bytes(4, "little") + u64(lamports) + u64(space) + owner
    return Ix(SYSTEM_PROGRAM, (Meta(payer, True, True), Meta(new, True, True)), data)


def initialize_mint_ix(mint: bytes, decimals: int, authority: bytes) -> Ix:
    """InitializeMint2 with freeze authority = None (never set)."""
    return Ix(TOKEN_PROGRAM, (Meta(mint, False, True),), bytes([20, decimals]) + authority + b"\x00" + bytes(32))


def associated_token_address(owner: bytes, mint: bytes) -> bytes:
    return find_program_address([owner, TOKEN_PROGRAM, mint], ATA_PROGRAM)[0]


def create_ata_idempotent_ix(payer: bytes, owner: bytes, mint: bytes) -> Ix:
    return Ix(ATA_PROGRAM, (
        Meta(payer, True, True), Meta(associated_token_address(owner, mint), False, True),
        Meta(owner, False, False), Meta(mint, False, False),
        Meta(SYSTEM_PROGRAM, False, False), Meta(TOKEN_PROGRAM, False, False),
    ), b"\x01")


def token_balance(rpc: Rpc, account: bytes) -> int:
    info = rpc.account(account)
    return int.from_bytes(info["raw"][64:72], "little") if info else 0
