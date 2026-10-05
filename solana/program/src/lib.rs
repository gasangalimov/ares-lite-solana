//! ARES Lite rewards program v4: FROZEN ECONOMICS
//! (ARES_LITE_FINAL_ECONOMICS_SPEC_v1 + R1 + R2).
//!
//! * `Initialize` (atomic genesis) mints EXACTLY 100,000,000 ARES (6 dp):
//!   10,000,000 into the founder VESTING vault (PDA token account) and
//!   90,000,000 into the MINING vault (PDA token account), then revokes the
//!   mint authority. The mint must have no freeze authority.
//! * Epochs: 24 h, index e = floor((now - genesis) / EPOCH). `CloseEpoch`
//!   pins the receipt-log head and the reward cap
//!   cap(e) = floor(A * 379_735 / 1_000_000_000), A = V - P - O.
//! * R1: P (HOLD) is a temporary reservation of the whole cap from close
//!   until `Finalize`/`Expire`. Only `Finalize` depletes the reserve
//!   permanently (by the awarded total). Failed/expired/zero epochs release
//!   their full cap.
//! * R2: one correction per epoch: `WithdrawResult` (before the original
//!   claim_open_ts) then `RepublishResult` before that same deadline, with
//!   total <= the original pinned cap. The original root/total stay on-chain.
//! * Claims only from FINAL epochs; per (epoch, claimant) receipt PDA.
//! * Bounties: 5% of the bounty goes to the mining vault (recycled), 95% to
//!   escrow; no burn. Founder vesting: 1-year cliff, then linear to year 5;
//!   released only by the founder beneficiary.
//!
//! There is no instruction that mints, burns, freezes, changes the economics,
//! or moves tokens out of the mining vault except `Claim`, or out of the
//! founder vault except `ReleaseVested`.

use solana_program::{
    account_info::{next_account_info, AccountInfo},
    clock::Clock,
    entrypoint::ProgramResult,
    hash::hashv,
    instruction::{AccountMeta, Instruction},
    msg,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    pubkey::Pubkey,
    rent::Rent,
    system_instruction, system_program,
    sysvar::Sysvar,
};

#[cfg(not(feature = "no-entrypoint"))]
solana_program::entrypoint!(process_instruction);

pub const PROGRAM_VERSION: u8 = 4;
pub const SPL_TOKEN_ID: Pubkey = solana_program::pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

// ------------------------------------------------------------ token economics

pub const DECIMALS: u8 = 6;
pub const UNIT: u64 = 1_000_000;
pub const MAX_SUPPLY: u64 = 100_000_000 * UNIT;
pub const FOUNDER_ALLOCATION: u64 = 10_000_000 * UNIT;
pub const MINING_ALLOCATION: u64 = 90_000_000 * UNIT;
pub const DECAY_NUM: u128 = 379_735;
pub const DECAY_DEN: u128 = 1_000_000_000;
pub const BOUNTY_FEE_DIVISOR: u64 = 20;
pub const MIN_BOUNTY: u64 = UNIT;

// --------------------------------------------------------------------- time
// Every duration is a fixed multiple of the epoch length. Production epoch =
// DAY. Durations in production seconds:
pub const DAY: i64 = 86_400;
pub const YEAR_EPOCHS: i64 = 365;
pub const COMMIT_END: i64 = 72_000;
pub const REVEAL_END: i64 = 79_200;
pub const CLOSE_GRACE: i64 = 21_600;
pub const PUBLISH_WINDOW: i64 = 172_800;
pub const CLAIM_DELAY: i64 = 259_200;

/// Only a build with the `devnet-time-scale` feature accepts an epoch shorter
/// than 24 h (a DEVNET rehearsal of multi-day/year flows). The production
/// build rejects any value other than DAY at genesis.
#[cfg(not(feature = "devnet-time-scale"))]
pub fn epoch_seconds_allowed(e: i64) -> bool {
    e == DAY
}
#[cfg(feature = "devnet-time-scale")]
pub fn epoch_seconds_allowed(e: i64) -> bool {
    e == DAY || (12..DAY).contains(&e) && e % 12 == 0
}

/// Scale a production duration to the configured epoch length (exact for
/// epoch_seconds multiple of 12; identity for epoch_seconds = DAY).
pub fn scaled(production_seconds: i64, epoch_seconds: i64) -> i64 {
    production_seconds * epoch_seconds / DAY
}

pub const CONFIG_SEED: &[u8] = b"config";
pub const VAULT_SEED: &[u8] = b"vault";
pub const VAULT_AUTHORITY_SEED: &[u8] = b"vault_authority";
pub const FOUNDER_VAULT_SEED: &[u8] = b"founder_vault";
pub const FOUNDER_AUTHORITY_SEED: &[u8] = b"founder_authority";
pub const EPOCH_SEED: &[u8] = b"epoch";
pub const CLAIM_SEED: &[u8] = b"claim";
pub const BOUNTY_SEED: &[u8] = b"bounty";
pub const ESCROW_SEED: &[u8] = b"escrow";

pub const CLAIM_LEAF_TAG: &[u8] = b"ARES-LITE/CLAIM-LEAF/v0";
pub const CLAIM_NODE_TAG: &[u8] = b"ARES-LITE/CLAIM-NODE/v0";
pub const MAX_PROOF_LEN: usize = 32;

const CONFIG_TAG: u8 = 41;
const EPOCH_TAG: u8 = 42;
const RECEIPT_TAG: u8 = 43;
const BOUNTY_TAG: u8 = 44;
pub const CONFIG_LEN: usize = 284;
pub const EPOCH_LEN: usize = 357;
pub const RECEIPT_LEN: usize = 50;
pub const BOUNTY_LEN: usize = 59;
const MINT_LEN: usize = 82;
const TOKEN_ACCOUNT_LEN: u64 = 165;
pub const NONE_EPOCH: u64 = u64::MAX;

pub const ST_OPEN: u8 = 0;
pub const ST_CLOSED: u8 = 1;
pub const ST_PUBLISHED: u8 = 2;
pub const ST_WITHDRAWN: u8 = 3;
pub const ST_CORRECTED: u8 = 4;
pub const ST_FINAL: u8 = 5;
pub const ST_EXPIRED: u8 = 6;
pub const BOUNTY_FUNDED: u8 = 0;
pub const BOUNTY_AWARDED: u8 = 1;

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiteError {
    InvalidInstruction = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    BadMint = 4,
    BadPda = 5,
    CapExceeded = 6,
    Overflow = 7,
    EpochExists = 8,
    RootAlreadyPublished = 9,
    NotFinal = 10,
    InvalidProof = 11,
    AlreadyClaimed = 12,
    BadAccount = 13,
    ZeroAmount = 14,
    GenesisInvariant = 15,
    BountyTooSmall = 16,
    BountyClosed = 17,
    WrongPhase = 18,
    LogHeadMismatch = 19,
    ClaimWindowNotOpen = 20,
    EpochCapExceeded = 21,
    OutsideCloseWindow = 22,
    EpochOrder = 23,
    DeadlinePassed = 24,
    DeadlineNotReached = 25,
    CorrectionUsed = 26,
    NotBeneficiary = 27,
    NothingVested = 28,
    ReserveUnderflow = 29,
    BadEpochSeconds = 30,
}

impl From<LiteError> for ProgramError {
    fn from(error: LiteError) -> Self {
        ProgramError::Custom(error as u32)
    }
}

// --------------------------------------------------------------- pure math

/// cap = floor(A * 379_735 / 1e9), u128 intermediate (A <= 2^64 → no overflow).
pub fn epoch_cap(available: u64) -> u64 {
    (available as u128 * DECAY_NUM / DECAY_DEN) as u64
}

/// A = V - P - O; None on underflow (never expected; aborts the instruction).
pub fn available(vault_balance: u64, pending: u64, outstanding: u64) -> Option<u64> {
    vault_balance.checked_sub(pending)?.checked_sub(outstanding)
}

/// Founder vesting: 0 before the cliff (1 year), linear to year 5, then all.
pub fn vested(genesis_ts: i64, epoch_seconds: i64, now_ts: i64) -> u64 {
    let year = YEAR_EPOCHS * epoch_seconds;
    let cliff = genesis_ts + year;
    let end = genesis_ts + 5 * year;
    if now_ts < cliff {
        0
    } else if now_ts >= end {
        FOUNDER_ALLOCATION
    } else {
        (FOUNDER_ALLOCATION as u128 * (now_ts - cliff) as u128 / (end - cliff) as u128) as u64
    }
}

/// (fee to the mining vault, escrow for the solver).
pub fn bounty_split(amount: u64) -> (u64, u64) {
    let fee = amount / BOUNTY_FEE_DIVISOR;
    (fee, amount - fee)
}

/// Start of epoch e; None on overflow.
pub fn epoch_start(genesis_ts: i64, epoch_seconds: i64, epoch: u64) -> Option<i64> {
    let e = i64::try_from(epoch).ok()?;
    genesis_ts.checked_add(e.checked_mul(epoch_seconds)?)
}

/// [start + REVEAL_END, start + EPOCH + CLOSE_GRACE)
pub fn close_window(genesis_ts: i64, epoch_seconds: i64, epoch: u64) -> Option<(i64, i64)> {
    let s = epoch_start(genesis_ts, epoch_seconds, epoch)?;
    Some((
        s.checked_add(scaled(REVEAL_END, epoch_seconds))?,
        s.checked_add(epoch_seconds)?.checked_add(scaled(CLOSE_GRACE, epoch_seconds))?,
    ))
}

// --------------------------------------------------------------- instructions

pub enum LiteInstruction {
    Initialize { founder_beneficiary: Pubkey, epoch_seconds: u64 },
    CreateEpoch { epoch: u64, manifest_hash: [u8; 32] },
    CloseEpoch { epoch: u64, log_head: [u8; 32], close_slot: u64 },
    PublishResult { epoch: u64, root: [u8; 32], total: u64, log_head: [u8; 32], digest: [u8; 32] },
    WithdrawResult { epoch: u64 },
    RepublishResult { epoch: u64, root: [u8; 32], total: u64, log_head: [u8; 32], digest: [u8; 32] },
    ExpireEpoch { epoch: u64 },
    FinalizeEpoch { epoch: u64 },
    Claim { epoch: u64, amount: u64, proof: Vec<[u8; 32]> },
    SetAdmin { new_admin: Pubkey },
    FundBounty { bounty_id: u64, amount: u64 },
    AwardBounty,
    ReleaseVested,
}

fn take<'a>(data: &mut &'a [u8], n: usize) -> Result<&'a [u8], ProgramError> {
    if data.len() < n {
        return Err(LiteError::InvalidInstruction.into());
    }
    let (head, tail) = data.split_at(n);
    *data = tail;
    Ok(head)
}

fn take_u64(data: &mut &[u8]) -> Result<u64, ProgramError> {
    Ok(u64::from_le_bytes(take(data, 8)?.try_into().unwrap()))
}

fn take_32(data: &mut &[u8]) -> Result<[u8; 32], ProgramError> {
    Ok(take(data, 32)?.try_into().unwrap())
}

impl LiteInstruction {
    pub fn unpack(input: &[u8]) -> Result<Self, ProgramError> {
        let mut data = input;
        let d = &mut data;
        let tag = take(d, 1)?[0];
        let instruction = match tag {
            0 => Self::Initialize { founder_beneficiary: Pubkey::new_from_array(take_32(d)?), epoch_seconds: take_u64(d)? },
            1 => Self::CreateEpoch { epoch: take_u64(d)?, manifest_hash: take_32(d)? },
            2 => Self::CloseEpoch { epoch: take_u64(d)?, log_head: take_32(d)?, close_slot: take_u64(d)? },
            3 => Self::PublishResult { epoch: take_u64(d)?, root: take_32(d)?, total: take_u64(d)?, log_head: take_32(d)?, digest: take_32(d)? },
            4 => Self::WithdrawResult { epoch: take_u64(d)? },
            5 => Self::RepublishResult { epoch: take_u64(d)?, root: take_32(d)?, total: take_u64(d)?, log_head: take_32(d)?, digest: take_32(d)? },
            6 => Self::ExpireEpoch { epoch: take_u64(d)? },
            7 => Self::FinalizeEpoch { epoch: take_u64(d)? },
            8 => {
                let epoch = take_u64(d)?;
                let amount = take_u64(d)?;
                let count = take(d, 1)?[0] as usize;
                if count > MAX_PROOF_LEN {
                    return Err(LiteError::InvalidInstruction.into());
                }
                let mut proof = Vec::with_capacity(count);
                for _ in 0..count {
                    proof.push(take_32(d)?);
                }
                Self::Claim { epoch, amount, proof }
            }
            9 => Self::SetAdmin { new_admin: Pubkey::new_from_array(take_32(d)?) },
            10 => Self::FundBounty { bounty_id: take_u64(d)?, amount: take_u64(d)? },
            11 => Self::AwardBounty,
            12 => Self::ReleaseVested,
            _ => return Err(LiteError::InvalidInstruction.into()),
        };
        if !data.is_empty() {
            return Err(LiteError::InvalidInstruction.into());
        }
        Ok(instruction)
    }
}

// ------------------------------------------------------------------ addresses

pub fn config_address(program_id: &Pubkey, mint: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[CONFIG_SEED, mint.as_ref()], program_id)
}
pub fn vault_address(program_id: &Pubkey, mint: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[VAULT_SEED, mint.as_ref()], program_id)
}
pub fn vault_authority_address(program_id: &Pubkey, mint: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[VAULT_AUTHORITY_SEED, mint.as_ref()], program_id)
}
pub fn founder_vault_address(program_id: &Pubkey, mint: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[FOUNDER_VAULT_SEED, mint.as_ref()], program_id)
}
pub fn founder_authority_address(program_id: &Pubkey, mint: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[FOUNDER_AUTHORITY_SEED, mint.as_ref()], program_id)
}
pub fn epoch_address(program_id: &Pubkey, config: &Pubkey, epoch: u64) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[EPOCH_SEED, config.as_ref(), &epoch.to_le_bytes()], program_id)
}
pub fn claim_address(program_id: &Pubkey, epoch_account: &Pubkey, claimant: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[CLAIM_SEED, epoch_account.as_ref(), claimant.as_ref()], program_id)
}
pub fn bounty_address(program_id: &Pubkey, config: &Pubkey, bounty_id: u64) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[BOUNTY_SEED, config.as_ref(), &bounty_id.to_le_bytes()], program_id)
}
pub fn escrow_address(program_id: &Pubkey, bounty: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[ESCROW_SEED, bounty.as_ref()], program_id)
}

// --------------------------------------------------------------------- merkle

pub fn leaf_hash(epoch: u64, claimant: &Pubkey, amount: u64) -> [u8; 32] {
    hashv(&[&[0u8], CLAIM_LEAF_TAG, &epoch.to_le_bytes(), claimant.as_ref(), &amount.to_le_bytes()]).to_bytes()
}

pub fn node_hash(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    let (low, high) = if a <= b { (a, b) } else { (b, a) };
    hashv(&[&[1u8], CLAIM_NODE_TAG, low, high]).to_bytes()
}

pub fn verify_proof(root: &[u8; 32], leaf: [u8; 32], proof: &[[u8; 32]]) -> bool {
    let mut node = leaf;
    for sibling in proof {
        node = node_hash(&node, sibling);
    }
    &node == root
}

// ------------------------------------------------------------------- accounts

fn rd_u64(data: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(data[at..at + 8].try_into().unwrap())
}
fn rd_key(data: &[u8], at: usize) -> Pubkey {
    Pubkey::new_from_array(data[at..at + 32].try_into().unwrap())
}
fn rd_32(data: &[u8], at: usize) -> [u8; 32] {
    data[at..at + 32].try_into().unwrap()
}
fn wr(data: &mut [u8], at: usize, bytes: &[u8]) {
    data[at..at + bytes.len()].copy_from_slice(bytes);
}

/// Config layout (offsets): 0 tag, 1 version, 2 admin, 34 mint, 66 vault,
/// 98 founder_vault, 130 founder_beneficiary, 162 genesis_ts, 170 epoch_seconds,
/// 178 pending_reserved (P), 186 outstanding_unclaimed (O), 194 last_closed_epoch,
/// 202 recycled_total, 210 mining_claimed_total, 218 founder_claimed,
/// 226 bounty_escrowed, 234 bounties_awarded, 242 mining_awarded_total,
/// 250 bump, 251 vault_bump, 252 vault_authority_bump, 253 founder_vault_bump,
/// 254 founder_authority_bump, 255..284 reserved (zero).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub version: u8,
    pub admin: Pubkey,
    pub mint: Pubkey,
    pub vault: Pubkey,
    pub founder_vault: Pubkey,
    pub founder_beneficiary: Pubkey,
    pub genesis_ts: i64,
    pub epoch_seconds: i64,
    pub pending_reserved: u64,
    pub outstanding_unclaimed: u64,
    pub last_closed_epoch: u64,
    pub recycled_total: u64,
    pub mining_claimed_total: u64,
    pub founder_claimed: u64,
    pub bounty_escrowed: u64,
    pub bounties_awarded: u64,
    /// Σ total over FINAL epochs (audit counter: the permanent depletion of R1).
    pub mining_awarded_total: u64,
    pub bump: u8,
    pub vault_bump: u8,
    pub vault_authority_bump: u8,
    pub founder_vault_bump: u8,
    pub founder_authority_bump: u8,
}

impl Config {
    pub fn unpack(data: &[u8]) -> Result<Self, ProgramError> {
        if data.len() != CONFIG_LEN || data[0] != CONFIG_TAG {
            return Err(LiteError::BadAccount.into());
        }
        Ok(Self {
            version: data[1],
            admin: rd_key(data, 2),
            mint: rd_key(data, 34),
            vault: rd_key(data, 66),
            founder_vault: rd_key(data, 98),
            founder_beneficiary: rd_key(data, 130),
            genesis_ts: rd_u64(data, 162) as i64,
            epoch_seconds: rd_u64(data, 170) as i64,
            pending_reserved: rd_u64(data, 178),
            outstanding_unclaimed: rd_u64(data, 186),
            last_closed_epoch: rd_u64(data, 194),
            recycled_total: rd_u64(data, 202),
            mining_claimed_total: rd_u64(data, 210),
            founder_claimed: rd_u64(data, 218),
            bounty_escrowed: rd_u64(data, 226),
            bounties_awarded: rd_u64(data, 234),
            mining_awarded_total: rd_u64(data, 242),
            bump: data[250],
            vault_bump: data[251],
            vault_authority_bump: data[252],
            founder_vault_bump: data[253],
            founder_authority_bump: data[254],
        })
    }

    fn pack(&self, data: &mut [u8]) {
        data[0] = CONFIG_TAG;
        data[1] = self.version;
        wr(data, 2, self.admin.as_ref());
        wr(data, 34, self.mint.as_ref());
        wr(data, 66, self.vault.as_ref());
        wr(data, 98, self.founder_vault.as_ref());
        wr(data, 130, self.founder_beneficiary.as_ref());
        wr(data, 162, &(self.genesis_ts as u64).to_le_bytes());
        wr(data, 170, &(self.epoch_seconds as u64).to_le_bytes());
        wr(data, 178, &self.pending_reserved.to_le_bytes());
        wr(data, 186, &self.outstanding_unclaimed.to_le_bytes());
        wr(data, 194, &self.last_closed_epoch.to_le_bytes());
        wr(data, 202, &self.recycled_total.to_le_bytes());
        wr(data, 210, &self.mining_claimed_total.to_le_bytes());
        wr(data, 218, &self.founder_claimed.to_le_bytes());
        wr(data, 226, &self.bounty_escrowed.to_le_bytes());
        wr(data, 234, &self.bounties_awarded.to_le_bytes());
        wr(data, 242, &self.mining_awarded_total.to_le_bytes());
        data[250] = self.bump;
        data[251] = self.vault_bump;
        data[252] = self.vault_authority_bump;
        data[253] = self.founder_vault_bump;
        data[254] = self.founder_authority_bump;
    }
}

/// Epoch layout (offsets): 0 tag, 1 status, 2 epoch, 10 manifest_hash, 42 log_head,
/// 74 close_slot, 82 cap, 90 root, 122 total, 130 digest, 162 claimed,
/// 170 deadline, 178 claim_open_ts, 186 orig_root, 218 orig_total, 226 orig_digest,
/// 258 corrections, 259 bump, 260 closed_ts, 268 published_ts, 276 corrected_ts,
/// 284 withdrawn_ts, 292..357 reserved (zero).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Epoch {
    pub status: u8,
    pub epoch: u64,
    pub manifest_hash: [u8; 32],
    pub log_head: [u8; 32],
    pub close_slot: u64,
    pub cap: u64,
    pub root: [u8; 32],
    pub total: u64,
    pub digest: [u8; 32],
    pub claimed: u64,
    /// CLOSED: publish deadline; WITHDRAWN: republish deadline (= original claim_open_ts).
    pub deadline: i64,
    pub claim_open_ts: i64,
    pub orig_root: [u8; 32],
    pub orig_total: u64,
    pub orig_digest: [u8; 32],
    pub corrections: u8,
    pub bump: u8,
    pub closed_ts: i64,
    pub published_ts: i64,
    pub corrected_ts: i64,
    pub withdrawn_ts: i64,
}

impl Epoch {
    pub fn unpack(data: &[u8]) -> Result<Self, ProgramError> {
        if data.len() != EPOCH_LEN || data[0] != EPOCH_TAG {
            return Err(LiteError::BadAccount.into());
        }
        Ok(Self {
            status: data[1],
            epoch: rd_u64(data, 2),
            manifest_hash: rd_32(data, 10),
            log_head: rd_32(data, 42),
            close_slot: rd_u64(data, 74),
            cap: rd_u64(data, 82),
            root: rd_32(data, 90),
            total: rd_u64(data, 122),
            digest: rd_32(data, 130),
            claimed: rd_u64(data, 162),
            deadline: rd_u64(data, 170) as i64,
            claim_open_ts: rd_u64(data, 178) as i64,
            orig_root: rd_32(data, 186),
            orig_total: rd_u64(data, 218),
            orig_digest: rd_32(data, 226),
            corrections: data[258],
            bump: data[259],
            closed_ts: rd_u64(data, 260) as i64,
            published_ts: rd_u64(data, 268) as i64,
            corrected_ts: rd_u64(data, 276) as i64,
            withdrawn_ts: rd_u64(data, 284) as i64,
        })
    }

    fn pack(&self, data: &mut [u8]) {
        data[0] = EPOCH_TAG;
        data[1] = self.status;
        wr(data, 2, &self.epoch.to_le_bytes());
        wr(data, 10, &self.manifest_hash);
        wr(data, 42, &self.log_head);
        wr(data, 74, &self.close_slot.to_le_bytes());
        wr(data, 82, &self.cap.to_le_bytes());
        wr(data, 90, &self.root);
        wr(data, 122, &self.total.to_le_bytes());
        wr(data, 130, &self.digest);
        wr(data, 162, &self.claimed.to_le_bytes());
        wr(data, 170, &(self.deadline as u64).to_le_bytes());
        wr(data, 178, &(self.claim_open_ts as u64).to_le_bytes());
        wr(data, 186, &self.orig_root);
        wr(data, 218, &self.orig_total.to_le_bytes());
        wr(data, 226, &self.orig_digest);
        data[258] = self.corrections;
        data[259] = self.bump;
        wr(data, 260, &(self.closed_ts as u64).to_le_bytes());
        wr(data, 268, &(self.published_ts as u64).to_le_bytes());
        wr(data, 276, &(self.corrected_ts as u64).to_le_bytes());
        wr(data, 284, &(self.withdrawn_ts as u64).to_le_bytes());
    }
}

struct MintView {
    mint_authority: Option<Pubkey>,
    supply: u64,
    decimals: u8,
    is_initialized: bool,
    freeze_authority: Option<Pubkey>,
}

fn read_coption_pubkey(data: &[u8]) -> Result<Option<Pubkey>, ProgramError> {
    match u32::from_le_bytes(data[0..4].try_into().unwrap()) {
        0 => Ok(None),
        1 => Ok(Some(Pubkey::new_from_array(data[4..36].try_into().unwrap()))),
        _ => Err(LiteError::BadMint.into()),
    }
}

fn read_mint(account: &AccountInfo) -> Result<MintView, ProgramError> {
    if account.owner != &SPL_TOKEN_ID {
        return Err(LiteError::BadMint.into());
    }
    let data = account.try_borrow_data()?;
    if data.len() != MINT_LEN {
        return Err(LiteError::BadMint.into());
    }
    Ok(MintView {
        mint_authority: read_coption_pubkey(&data[0..36])?,
        supply: u64::from_le_bytes(data[36..44].try_into().unwrap()),
        decimals: data[44],
        is_initialized: data[45] == 1,
        freeze_authority: read_coption_pubkey(&data[46..82])?,
    })
}

/// (mint, owner, amount) of a classic SPL token account.
fn read_token_account(account: &AccountInfo) -> Result<(Pubkey, Pubkey, u64), ProgramError> {
    if account.owner != &SPL_TOKEN_ID {
        return Err(LiteError::BadAccount.into());
    }
    let data = account.try_borrow_data()?;
    if data.len() != TOKEN_ACCOUNT_LEN as usize {
        return Err(LiteError::BadAccount.into());
    }
    Ok((rd_key(&data, 0), rd_key(&data, 32), rd_u64(&data, 64)))
}

// -------------------------------------------------------------------- helpers

fn require_signer(account: &AccountInfo) -> ProgramResult {
    if !account.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    Ok(())
}

fn require_key(account: &AccountInfo, expected: &Pubkey) -> ProgramResult {
    if account.key != expected {
        return Err(LiteError::BadPda.into());
    }
    Ok(())
}

/// Create a PDA account owned by `owner`, tolerating pre-funded addresses.
fn create_pda<'a>(
    payer: &AccountInfo<'a>,
    target: &AccountInfo<'a>,
    system: &AccountInfo<'a>,
    owner: &Pubkey,
    space: usize,
    seeds: &[&[u8]],
) -> ProgramResult {
    let required = Rent::get()?.minimum_balance(space);
    if target.lamports() == 0 {
        invoke_signed(
            &system_instruction::create_account(payer.key, target.key, required, space as u64, owner),
            &[payer.clone(), target.clone(), system.clone()],
            &[seeds],
        )
    } else {
        let top_up = required.saturating_sub(target.lamports());
        if top_up > 0 {
            invoke(&system_instruction::transfer(payer.key, target.key, top_up), &[payer.clone(), target.clone(), system.clone()])?;
        }
        invoke_signed(&system_instruction::allocate(target.key, space as u64), &[target.clone(), system.clone()], &[seeds])?;
        invoke_signed(&system_instruction::assign(target.key, owner), &[target.clone(), system.clone()], &[seeds])
    }
}

fn token_ix(data: Vec<u8>, accounts: Vec<AccountMeta>) -> Instruction {
    Instruction { program_id: SPL_TOKEN_ID, accounts, data }
}

fn amount_ix(tag: u8, amount: u64) -> Vec<u8> {
    let mut data = vec![tag];
    data.extend_from_slice(&amount.to_le_bytes());
    data
}

fn init_token_account<'a>(account: &AccountInfo<'a>, mint: &AccountInfo<'a>, token: &AccountInfo<'a>, owner: &Pubkey) -> ProgramResult {
    let mut init = vec![18u8]; // InitializeAccount3
    init.extend_from_slice(owner.as_ref());
    invoke(
        &token_ix(init, vec![AccountMeta::new(*account.key, false), AccountMeta::new_readonly(*mint.key, false)]),
        &[account.clone(), mint.clone(), token.clone()],
    )
}

/// SPL Transfer signed by the token owner (a user), not by the program.
fn user_transfer<'a>(owner: &AccountInfo<'a>, from: &AccountInfo<'a>, to: &AccountInfo<'a>, token: &AccountInfo<'a>, amount: u64) -> ProgramResult {
    invoke(
        &token_ix(
            amount_ix(3, amount),
            vec![AccountMeta::new(*from.key, false), AccountMeta::new(*to.key, false), AccountMeta::new_readonly(*owner.key, true)],
        ),
        &[from.clone(), to.clone(), owner.clone(), token.clone()],
    )
}

fn load_config(program_id: &Pubkey, account: &AccountInfo) -> Result<Config, ProgramError> {
    if account.owner != program_id {
        return Err(LiteError::BadAccount.into());
    }
    let config = Config::unpack(&account.try_borrow_data()?)?;
    let expected = Pubkey::create_program_address(&[CONFIG_SEED, config.mint.as_ref(), &[config.bump]], program_id)
        .map_err(|_| ProgramError::from(LiteError::BadPda))?;
    require_key(account, &expected)?;
    Ok(config)
}

fn load_epoch(program_id: &Pubkey, config_key: &Pubkey, account: &AccountInfo, epoch: u64) -> Result<Epoch, ProgramError> {
    if account.owner != program_id {
        return Err(LiteError::BadAccount.into());
    }
    let ep = Epoch::unpack(&account.try_borrow_data()?)?;
    if ep.epoch != epoch {
        return Err(LiteError::BadAccount.into());
    }
    let expected =
        Pubkey::create_program_address(&[EPOCH_SEED, config_key.as_ref(), &ep.epoch.to_le_bytes(), &[ep.bump]], program_id)
            .map_err(|_| ProgramError::from(LiteError::BadPda))?;
    require_key(account, &expected)?;
    Ok(ep)
}

fn require_admin(config: &Config, admin: &AccountInfo) -> ProgramResult {
    require_signer(admin)?;
    if config.admin != *admin.key {
        return Err(LiteError::NotAdmin.into());
    }
    Ok(())
}

fn now() -> Result<i64, ProgramError> {
    Ok(Clock::get()?.unix_timestamp)
}

fn hex32(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ----------------------------------------------------------------- processors

pub fn process_instruction(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    match LiteInstruction::unpack(data)? {
        LiteInstruction::Initialize { founder_beneficiary, epoch_seconds } => {
            initialize(program_id, accounts, founder_beneficiary, epoch_seconds)
        }
        LiteInstruction::CreateEpoch { epoch, manifest_hash } => create_epoch(program_id, accounts, epoch, manifest_hash),
        LiteInstruction::CloseEpoch { epoch, log_head, close_slot } => close_epoch(program_id, accounts, epoch, log_head, close_slot),
        LiteInstruction::PublishResult { epoch, root, total, log_head, digest } => {
            publish(program_id, accounts, epoch, root, total, log_head, digest, false)
        }
        LiteInstruction::WithdrawResult { epoch } => withdraw(program_id, accounts, epoch),
        LiteInstruction::RepublishResult { epoch, root, total, log_head, digest } => {
            publish(program_id, accounts, epoch, root, total, log_head, digest, true)
        }
        LiteInstruction::ExpireEpoch { epoch } => expire(program_id, accounts, epoch),
        LiteInstruction::FinalizeEpoch { epoch } => finalize(program_id, accounts, epoch),
        LiteInstruction::Claim { epoch, amount, proof } => claim(program_id, accounts, epoch, amount, &proof),
        LiteInstruction::SetAdmin { new_admin } => set_admin(program_id, accounts, new_admin),
        LiteInstruction::FundBounty { bounty_id, amount } => fund_bounty(program_id, accounts, bounty_id, amount),
        LiteInstruction::AwardBounty => award_bounty(program_id, accounts),
        LiteInstruction::ReleaseVested => release_vested(program_id, accounts),
    }
}

/// Atomic genesis.
/// Accounts: [deployer (signer, w; current mint authority, becomes admin), config (w),
/// mint (w), mining vault (w), vault authority, founder vault (w), founder authority,
/// system, token].
fn initialize(program_id: &Pubkey, accounts: &[AccountInfo], founder_beneficiary: Pubkey, epoch_seconds: u64) -> ProgramResult {
    let iter = &mut accounts.iter();
    let deployer = next_account_info(iter)?;
    let config_account = next_account_info(iter)?;
    let mint = next_account_info(iter)?;
    let vault = next_account_info(iter)?;
    let vault_authority = next_account_info(iter)?;
    let founder_vault = next_account_info(iter)?;
    let founder_authority = next_account_info(iter)?;
    let system = next_account_info(iter)?;
    let token = next_account_info(iter)?;
    require_signer(deployer)?;
    require_key(system, &system_program::ID)?;
    require_key(token, &SPL_TOKEN_ID)?;
    let epoch_seconds = i64::try_from(epoch_seconds).map_err(|_| ProgramError::from(LiteError::BadEpochSeconds))?;
    if !epoch_seconds_allowed(epoch_seconds) {
        return Err(LiteError::BadEpochSeconds.into());
    }
    if founder_beneficiary == Pubkey::default() {
        return Err(LiteError::InvalidInstruction.into());
    }

    let (config_key, bump) = config_address(program_id, mint.key);
    require_key(config_account, &config_key)?;
    if config_account.owner == program_id || !config_account.data_is_empty() {
        return Err(LiteError::AlreadyInitialized.into());
    }
    let view = read_mint(mint)?;
    if !view.is_initialized
        || view.supply != 0
        || view.decimals != DECIMALS
        || view.freeze_authority.is_some()
        || view.mint_authority != Some(*deployer.key)
    {
        return Err(LiteError::BadMint.into());
    }
    let (vault_key, vault_bump) = vault_address(program_id, mint.key);
    let (authority_key, authority_bump) = vault_authority_address(program_id, mint.key);
    let (fvault_key, fvault_bump) = founder_vault_address(program_id, mint.key);
    let (fauth_key, fauth_bump) = founder_authority_address(program_id, mint.key);
    require_key(vault, &vault_key)?;
    require_key(vault_authority, &authority_key)?;
    require_key(founder_vault, &fvault_key)?;
    require_key(founder_authority, &fauth_key)?;

    create_pda(deployer, config_account, system, program_id, CONFIG_LEN, &[CONFIG_SEED, mint.key.as_ref(), &[bump]])?;
    create_pda(deployer, vault, system, &SPL_TOKEN_ID, TOKEN_ACCOUNT_LEN as usize, &[VAULT_SEED, mint.key.as_ref(), &[vault_bump]])?;
    init_token_account(vault, mint, token, &authority_key)?;
    create_pda(
        deployer,
        founder_vault,
        system,
        &SPL_TOKEN_ID,
        TOKEN_ACCOUNT_LEN as usize,
        &[FOUNDER_VAULT_SEED, mint.key.as_ref(), &[fvault_bump]],
    )?;
    init_token_account(founder_vault, mint, token, &fauth_key)?;
    let mint_meta =
        |to: &Pubkey| vec![AccountMeta::new(*mint.key, false), AccountMeta::new(*to, false), AccountMeta::new_readonly(*deployer.key, true)];
    invoke(&token_ix(amount_ix(7, MINING_ALLOCATION), mint_meta(&vault_key)), &[mint.clone(), vault.clone(), deployer.clone(), token.clone()])?;
    invoke(
        &token_ix(amount_ix(7, FOUNDER_ALLOCATION), mint_meta(&fvault_key)),
        &[mint.clone(), founder_vault.clone(), deployer.clone(), token.clone()],
    )?;
    // Revoke the mint authority forever: SetAuthority(MintTokens, None).
    invoke(
        &token_ix(vec![6u8, 0u8, 0u8], vec![AccountMeta::new(*mint.key, false), AccountMeta::new_readonly(*deployer.key, true)]),
        &[mint.clone(), deployer.clone(), token.clone()],
    )?;
    let after = read_mint(mint)?;
    let (_, _, vault_balance) = read_token_account(vault)?;
    let (_, _, founder_balance) = read_token_account(founder_vault)?;
    if after.supply != MAX_SUPPLY
        || after.mint_authority.is_some()
        || after.freeze_authority.is_some()
        || vault_balance != MINING_ALLOCATION
        || founder_balance != FOUNDER_ALLOCATION
        || FOUNDER_ALLOCATION + MINING_ALLOCATION != MAX_SUPPLY
    {
        return Err(LiteError::GenesisInvariant.into());
    }

    Config {
        version: PROGRAM_VERSION,
        admin: *deployer.key,
        mint: *mint.key,
        vault: vault_key,
        founder_vault: fvault_key,
        founder_beneficiary,
        genesis_ts: now()?,
        epoch_seconds,
        pending_reserved: 0,
        outstanding_unclaimed: 0,
        last_closed_epoch: NONE_EPOCH,
        recycled_total: 0,
        mining_claimed_total: 0,
        founder_claimed: 0,
        bounty_escrowed: 0,
        bounties_awarded: 0,
        mining_awarded_total: 0,
        bump,
        vault_bump,
        vault_authority_bump: authority_bump,
        founder_vault_bump: fvault_bump,
        founder_authority_bump: fauth_bump,
    }
    .pack(&mut config_account.try_borrow_mut_data()?);
    msg!("ARES genesis: supply {} fixed; founder vesting {}; mining {}; epoch_seconds {}", MAX_SUPPLY, FOUNDER_ALLOCATION, MINING_ALLOCATION, epoch_seconds);
    Ok(())
}

/// Accounts: [admin (signer, w), config, epoch (w), system].
fn create_epoch(program_id: &Pubkey, accounts: &[AccountInfo], epoch: u64, manifest_hash: [u8; 32]) -> ProgramResult {
    let iter = &mut accounts.iter();
    let admin = next_account_info(iter)?;
    let config_account = next_account_info(iter)?;
    let epoch_account = next_account_info(iter)?;
    let system = next_account_info(iter)?;
    require_key(system, &system_program::ID)?;
    let config = load_config(program_id, config_account)?;
    require_admin(&config, admin)?;
    let (key, bump) = epoch_address(program_id, config_account.key, epoch);
    require_key(epoch_account, &key)?;
    if epoch_account.owner == program_id || !epoch_account.data_is_empty() {
        return Err(LiteError::EpochExists.into());
    }
    create_pda(admin, epoch_account, system, program_id, EPOCH_LEN, &[EPOCH_SEED, config_account.key.as_ref(), &epoch.to_le_bytes(), &[bump]])?;
    Epoch {
        status: ST_OPEN,
        epoch,
        manifest_hash,
        log_head: [0; 32],
        close_slot: 0,
        cap: 0,
        root: [0; 32],
        total: 0,
        digest: [0; 32],
        claimed: 0,
        deadline: 0,
        claim_open_ts: 0,
        orig_root: [0; 32],
        orig_total: 0,
        orig_digest: [0; 32],
        corrections: 0,
        bump,
        closed_ts: 0,
        published_ts: 0,
        corrected_ts: 0,
        withdrawn_ts: 0,
    }
    .pack(&mut epoch_account.try_borrow_mut_data()?);
    Ok(())
}

/// Accounts: [admin (signer), config (w), epoch (w), mining vault].
/// Pins the receipt-log head and cap(e) = floor(A * DECAY) with A = V - P - O.
/// Only inside [start + REVEAL_END, start + EPOCH + CLOSE_GRACE); indices
/// strictly increase. P += cap (temporary HOLD, R1).
fn close_epoch(program_id: &Pubkey, accounts: &[AccountInfo], epoch: u64, log_head: [u8; 32], close_slot: u64) -> ProgramResult {
    let iter = &mut accounts.iter();
    let admin = next_account_info(iter)?;
    let config_account = next_account_info(iter)?;
    let epoch_account = next_account_info(iter)?;
    let vault = next_account_info(iter)?;
    let mut config = load_config(program_id, config_account)?;
    require_admin(&config, admin)?;
    require_key(vault, &config.vault)?;
    let mut ep = load_epoch(program_id, config_account.key, epoch_account, epoch)?;
    if ep.status != ST_OPEN {
        return Err(LiteError::WrongPhase.into());
    }
    if config.last_closed_epoch != NONE_EPOCH && epoch <= config.last_closed_epoch {
        return Err(LiteError::EpochOrder.into());
    }
    let now = now()?;
    let (open, end) = close_window(config.genesis_ts, config.epoch_seconds, epoch).ok_or(LiteError::Overflow)?;
    if now < open || now >= end {
        return Err(LiteError::OutsideCloseWindow.into());
    }
    let (_, _, vault_balance) = read_token_account(vault)?;
    let avail = available(vault_balance, config.pending_reserved, config.outstanding_unclaimed).ok_or(LiteError::ReserveUnderflow)?;
    let cap = epoch_cap(avail);
    config.pending_reserved = config.pending_reserved.checked_add(cap).ok_or(LiteError::Overflow)?;
    config.last_closed_epoch = epoch;
    ep.log_head = log_head;
    ep.close_slot = close_slot;
    ep.cap = cap;
    ep.closed_ts = now;
    ep.deadline = now.checked_add(scaled(PUBLISH_WINDOW, config.epoch_seconds)).ok_or(LiteError::Overflow)?;
    ep.status = ST_CLOSED;
    ep.pack(&mut epoch_account.try_borrow_mut_data()?);
    config.pack(&mut config_account.try_borrow_mut_data()?);
    msg!("ARES:EpochClosed epoch={} available={} cap={} P={}", epoch, avail, cap, config.pending_reserved);
    Ok(())
}

/// Accounts: [admin (signer), config, epoch (w)].
/// Publish: CLOSED → PUBLISHED before the publish deadline.
/// Republish (R2): WITHDRAWN → CORRECTED before the republish deadline.
/// P and O do not change: the whole cap stays reserved until FINAL.
#[allow(clippy::too_many_arguments)]
fn publish(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    epoch: u64,
    root: [u8; 32],
    total: u64,
    log_head: [u8; 32],
    digest: [u8; 32],
    correction: bool,
) -> ProgramResult {
    let iter = &mut accounts.iter();
    let admin = next_account_info(iter)?;
    let config_account = next_account_info(iter)?;
    let epoch_account = next_account_info(iter)?;
    let config = load_config(program_id, config_account)?;
    require_admin(&config, admin)?;
    let mut ep = load_epoch(program_id, config_account.key, epoch_account, epoch)?;
    let phase_error = match (correction, ep.status) {
        (false, ST_CLOSED) | (true, ST_WITHDRAWN) => None,
        (_, ST_EXPIRED) => Some(LiteError::DeadlinePassed),
        (false, ST_OPEN) | (true, ST_OPEN) | (true, ST_CLOSED) => Some(LiteError::WrongPhase),
        (false, _) => Some(LiteError::RootAlreadyPublished),
        (true, ST_PUBLISHED) => Some(LiteError::WrongPhase), // must withdraw first
        (true, _) => Some(LiteError::CorrectionUsed),
    };
    if let Some(error) = phase_error {
        return Err(error.into());
    }
    let now = now()?;
    if now >= ep.deadline {
        return Err(LiteError::DeadlinePassed.into());
    }
    if ep.log_head != log_head {
        return Err(LiteError::LogHeadMismatch.into());
    }
    if total > ep.cap {
        return Err(LiteError::EpochCapExceeded.into());
    }
    ep.root = root;
    ep.total = total;
    ep.digest = digest;
    ep.deadline = 0;
    ep.claim_open_ts = now.checked_add(scaled(CLAIM_DELAY, config.epoch_seconds)).ok_or(LiteError::Overflow)?;
    if correction {
        ep.corrections = 1;
        ep.corrected_ts = now;
        ep.status = ST_CORRECTED;
        msg!(
            "ARES:ResultCorrected epoch={} orig_root={} orig_total={} root={} total={} claim_open_ts={}",
            epoch,
            hex32(&ep.orig_root),
            ep.orig_total,
            hex32(&root),
            total,
            ep.claim_open_ts
        );
    } else {
        ep.published_ts = now;
        ep.status = ST_PUBLISHED;
        msg!("ARES:ResultPublished epoch={} root={} total={} cap={} claim_open_ts={}", epoch, hex32(&root), total, ep.cap, ep.claim_open_ts);
    }
    ep.pack(&mut epoch_account.try_borrow_mut_data()?);
    Ok(())
}

/// Accounts: [admin (signer), config, epoch (w)].
/// R2: PUBLISHED → WITHDRAWN, once, strictly before claim_open_ts. The original
/// root/total/digest are kept; the republish deadline is the ORIGINAL
/// claim_open_ts (no extension). No P/O change.
fn withdraw(program_id: &Pubkey, accounts: &[AccountInfo], epoch: u64) -> ProgramResult {
    let iter = &mut accounts.iter();
    let admin = next_account_info(iter)?;
    let config_account = next_account_info(iter)?;
    let epoch_account = next_account_info(iter)?;
    let config = load_config(program_id, config_account)?;
    require_admin(&config, admin)?;
    let mut ep = load_epoch(program_id, config_account.key, epoch_account, epoch)?;
    if ep.corrections != 0 {
        return Err(LiteError::CorrectionUsed.into());
    }
    if ep.status != ST_PUBLISHED {
        return Err(LiteError::WrongPhase.into());
    }
    let now = now()?;
    if now >= ep.claim_open_ts {
        return Err(LiteError::DeadlinePassed.into());
    }
    ep.orig_root = ep.root;
    ep.orig_total = ep.total;
    ep.orig_digest = ep.digest;
    ep.root = [0; 32];
    ep.total = 0;
    ep.digest = [0; 32];
    ep.deadline = ep.claim_open_ts;
    ep.claim_open_ts = 0;
    ep.withdrawn_ts = now;
    ep.corrections = 1;
    ep.status = ST_WITHDRAWN;
    ep.pack(&mut epoch_account.try_borrow_mut_data()?);
    msg!("ARES:ResultWithdrawn epoch={} orig_root={} orig_total={} republish_deadline={}", epoch, hex32(&ep.orig_root), ep.orig_total, ep.deadline);
    Ok(())
}

/// Permissionless. Accounts: [config (w), epoch (w)].
/// CLOSED or WITHDRAWN past its deadline → EXPIRED; P -= cap (no depletion).
fn expire(program_id: &Pubkey, accounts: &[AccountInfo], epoch: u64) -> ProgramResult {
    let iter = &mut accounts.iter();
    let config_account = next_account_info(iter)?;
    let epoch_account = next_account_info(iter)?;
    let mut config = load_config(program_id, config_account)?;
    let mut ep = load_epoch(program_id, config_account.key, epoch_account, epoch)?;
    if ep.status != ST_CLOSED && ep.status != ST_WITHDRAWN {
        return Err(LiteError::WrongPhase.into());
    }
    if now()? < ep.deadline {
        return Err(LiteError::DeadlineNotReached.into());
    }
    config.pending_reserved = config.pending_reserved.checked_sub(ep.cap).ok_or(LiteError::Overflow)?;
    ep.total = 0;
    ep.status = ST_EXPIRED;
    ep.pack(&mut epoch_account.try_borrow_mut_data()?);
    config.pack(&mut config_account.try_borrow_mut_data()?);
    msg!("ARES:EpochExpired epoch={} released={}", epoch, ep.cap);
    Ok(())
}

/// Permissionless. Accounts: [config (w), epoch (w)].
/// PUBLISHED/CORRECTED at or after claim_open_ts → FINAL:
/// P -= cap; O += total (the only permanent depletion, R1).
fn finalize(program_id: &Pubkey, accounts: &[AccountInfo], epoch: u64) -> ProgramResult {
    let iter = &mut accounts.iter();
    let config_account = next_account_info(iter)?;
    let epoch_account = next_account_info(iter)?;
    let mut config = load_config(program_id, config_account)?;
    let mut ep = load_epoch(program_id, config_account.key, epoch_account, epoch)?;
    if ep.status != ST_PUBLISHED && ep.status != ST_CORRECTED {
        return Err(LiteError::WrongPhase.into());
    }
    if now()? < ep.claim_open_ts {
        return Err(LiteError::ClaimWindowNotOpen.into());
    }
    config.pending_reserved = config.pending_reserved.checked_sub(ep.cap).ok_or(LiteError::Overflow)?;
    config.outstanding_unclaimed = config.outstanding_unclaimed.checked_add(ep.total).ok_or(LiteError::Overflow)?;
    config.mining_awarded_total = config.mining_awarded_total.checked_add(ep.total).ok_or(LiteError::Overflow)?;
    ep.status = ST_FINAL;
    ep.pack(&mut epoch_account.try_borrow_mut_data()?);
    config.pack(&mut config_account.try_borrow_mut_data()?);
    msg!("ARES:EpochFinal epoch={} total={} released={}", epoch, ep.total, ep.cap - ep.total);
    Ok(())
}

/// Accounts: [claimant (signer, w), config (w), epoch (w), receipt (w), mining vault (w),
/// destination token account (w, owned by claimant), vault authority, token, system].
fn claim(program_id: &Pubkey, accounts: &[AccountInfo], epoch: u64, amount: u64, proof: &[[u8; 32]]) -> ProgramResult {
    let iter = &mut accounts.iter();
    let claimant = next_account_info(iter)?;
    let config_account = next_account_info(iter)?;
    let epoch_account = next_account_info(iter)?;
    let receipt = next_account_info(iter)?;
    let vault = next_account_info(iter)?;
    let destination = next_account_info(iter)?;
    let vault_authority = next_account_info(iter)?;
    let token = next_account_info(iter)?;
    let system = next_account_info(iter)?;
    require_signer(claimant)?;
    require_key(token, &SPL_TOKEN_ID)?;
    require_key(system, &system_program::ID)?;
    if amount == 0 {
        return Err(LiteError::ZeroAmount.into());
    }
    let mut config = load_config(program_id, config_account)?;
    require_key(vault, &config.vault)?;
    let authority_key =
        Pubkey::create_program_address(&[VAULT_AUTHORITY_SEED, config.mint.as_ref(), &[config.vault_authority_bump]], program_id)
            .map_err(|_| ProgramError::from(LiteError::BadPda))?;
    require_key(vault_authority, &authority_key)?;
    let (dest_mint, dest_owner, _) = read_token_account(destination)?;
    if dest_mint != config.mint || dest_owner != *claimant.key {
        return Err(LiteError::BadAccount.into());
    }
    let mut ep = load_epoch(program_id, config_account.key, epoch_account, epoch)?;
    if ep.status != ST_FINAL {
        return Err(LiteError::NotFinal.into());
    }
    if !verify_proof(&ep.root, leaf_hash(ep.epoch, claimant.key, amount), proof) {
        return Err(LiteError::InvalidProof.into());
    }
    let (receipt_key, receipt_bump) = claim_address(program_id, epoch_account.key, claimant.key);
    require_key(receipt, &receipt_key)?;
    if receipt.owner == program_id || !receipt.data_is_empty() {
        return Err(LiteError::AlreadyClaimed.into());
    }
    let claimed = ep.claimed.checked_add(amount).ok_or(LiteError::Overflow)?;
    if claimed > ep.total {
        return Err(LiteError::CapExceeded.into());
    }
    config.outstanding_unclaimed = config.outstanding_unclaimed.checked_sub(amount).ok_or(LiteError::CapExceeded)?;
    config.mining_claimed_total = config.mining_claimed_total.checked_add(amount).ok_or(LiteError::Overflow)?;
    create_pda(claimant, receipt, system, program_id, RECEIPT_LEN, &[CLAIM_SEED, epoch_account.key.as_ref(), claimant.key.as_ref(), &[receipt_bump]])?;
    {
        let mut data = receipt.try_borrow_mut_data()?;
        data[0] = RECEIPT_TAG;
        wr(&mut data, 1, &ep.epoch.to_le_bytes());
        wr(&mut data, 9, claimant.key.as_ref());
        wr(&mut data, 41, &amount.to_le_bytes());
        data[49] = receipt_bump;
    }
    ep.claimed = claimed;
    ep.pack(&mut epoch_account.try_borrow_mut_data()?);
    config.pack(&mut config_account.try_borrow_mut_data()?);
    invoke_signed(
        &token_ix(
            amount_ix(3, amount),
            vec![AccountMeta::new(*vault.key, false), AccountMeta::new(*destination.key, false), AccountMeta::new_readonly(authority_key, true)],
        ),
        &[vault.clone(), destination.clone(), vault_authority.clone(), token.clone()],
        &[&[VAULT_AUTHORITY_SEED, config.mint.as_ref(), &[config.vault_authority_bump]]],
    )
}

/// Accounts: [admin (signer), config (w)].
fn set_admin(program_id: &Pubkey, accounts: &[AccountInfo], new_admin: Pubkey) -> ProgramResult {
    let iter = &mut accounts.iter();
    let admin = next_account_info(iter)?;
    let config_account = next_account_info(iter)?;
    let mut config = load_config(program_id, config_account)?;
    require_admin(&config, admin)?;
    config.admin = new_admin;
    config.pack(&mut config_account.try_borrow_mut_data()?);
    Ok(())
}

/// Accounts: [funder (signer, w), funder token account (w), config (w), mint,
/// mining vault (w), bounty (w, PDA), escrow (w, PDA token account), system, token].
/// fee = floor(B/20) → mining vault (recycled_total += fee); B - fee → escrow.
fn fund_bounty(program_id: &Pubkey, accounts: &[AccountInfo], bounty_id: u64, amount: u64) -> ProgramResult {
    let iter = &mut accounts.iter();
    let funder = next_account_info(iter)?;
    let funder_account = next_account_info(iter)?;
    let config_account = next_account_info(iter)?;
    let mint = next_account_info(iter)?;
    let vault = next_account_info(iter)?;
    let bounty = next_account_info(iter)?;
    let escrow = next_account_info(iter)?;
    let system = next_account_info(iter)?;
    let token = next_account_info(iter)?;
    require_signer(funder)?;
    require_key(system, &system_program::ID)?;
    require_key(token, &SPL_TOKEN_ID)?;
    if amount < MIN_BOUNTY {
        return Err(LiteError::BountyTooSmall.into());
    }
    let mut config = load_config(program_id, config_account)?;
    require_key(mint, &config.mint)?;
    require_key(vault, &config.vault)?;
    let (bounty_key, bounty_bump) = bounty_address(program_id, config_account.key, bounty_id);
    require_key(bounty, &bounty_key)?;
    if bounty.owner == program_id || !bounty.data_is_empty() {
        return Err(LiteError::AlreadyInitialized.into());
    }
    let (escrow_key, escrow_bump) = escrow_address(program_id, &bounty_key);
    require_key(escrow, &escrow_key)?;
    let (fee, reward) = bounty_split(amount);

    create_pda(funder, bounty, system, program_id, BOUNTY_LEN, &[BOUNTY_SEED, config_account.key.as_ref(), &bounty_id.to_le_bytes(), &[bounty_bump]])?;
    create_pda(funder, escrow, system, &SPL_TOKEN_ID, TOKEN_ACCOUNT_LEN as usize, &[ESCROW_SEED, bounty_key.as_ref(), &[escrow_bump]])?;
    init_token_account(escrow, mint, token, &bounty_key)?;
    user_transfer(funder, funder_account, vault, token, fee)?; // fee >= 50_000 since amount >= MIN_BOUNTY
    user_transfer(funder, funder_account, escrow, token, reward)?;
    {
        let mut data = bounty.try_borrow_mut_data()?;
        data[0] = BOUNTY_TAG;
        data[1] = BOUNTY_FUNDED;
        wr(&mut data, 2, &bounty_id.to_le_bytes());
        wr(&mut data, 10, funder.key.as_ref());
        wr(&mut data, 42, &reward.to_le_bytes());
        wr(&mut data, 50, &fee.to_le_bytes());
        data[58] = bounty_bump;
    }
    config.recycled_total = config.recycled_total.checked_add(fee).ok_or(LiteError::Overflow)?;
    config.bounty_escrowed = config.bounty_escrowed.checked_add(reward).ok_or(LiteError::Overflow)?;
    config.pack(&mut config_account.try_borrow_mut_data()?);
    msg!("ARES:BountyFunded id={} amount={} recycled={} escrow={}", bounty_id, amount, fee, reward);
    Ok(())
}

/// Accounts: [admin (signer), config (w), bounty (w), escrow (w), destination (w), token].
fn award_bounty(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let iter = &mut accounts.iter();
    let admin = next_account_info(iter)?;
    let config_account = next_account_info(iter)?;
    let bounty = next_account_info(iter)?;
    let escrow = next_account_info(iter)?;
    let destination = next_account_info(iter)?;
    let token = next_account_info(iter)?;
    require_key(token, &SPL_TOKEN_ID)?;
    let mut config = load_config(program_id, config_account)?;
    require_admin(&config, admin)?;
    if bounty.owner != program_id {
        return Err(LiteError::BadAccount.into());
    }
    let (status, bounty_id, reward, bump) = {
        let data = bounty.try_borrow_data()?;
        if data.len() != BOUNTY_LEN || data[0] != BOUNTY_TAG {
            return Err(LiteError::BadAccount.into());
        }
        (data[1], rd_u64(&data, 2), rd_u64(&data, 42), data[58])
    };
    let expected = Pubkey::create_program_address(&[BOUNTY_SEED, config_account.key.as_ref(), &bounty_id.to_le_bytes(), &[bump]], program_id)
        .map_err(|_| ProgramError::from(LiteError::BadPda))?;
    require_key(bounty, &expected)?;
    require_key(escrow, &escrow_address(program_id, bounty.key).0)?;
    if status != BOUNTY_FUNDED {
        return Err(LiteError::BountyClosed.into());
    }
    bounty.try_borrow_mut_data()?[1] = BOUNTY_AWARDED;
    config.bounty_escrowed = config.bounty_escrowed.checked_sub(reward).ok_or(LiteError::Overflow)?;
    config.bounties_awarded = config.bounties_awarded.checked_add(1).ok_or(LiteError::Overflow)?;
    config.pack(&mut config_account.try_borrow_mut_data()?);
    invoke_signed(
        &token_ix(
            amount_ix(3, reward),
            vec![AccountMeta::new(*escrow.key, false), AccountMeta::new(*destination.key, false), AccountMeta::new_readonly(*bounty.key, true)],
        ),
        &[escrow.clone(), destination.clone(), bounty.clone(), token.clone()],
        &[&[BOUNTY_SEED, config_account.key.as_ref(), &bounty_id.to_le_bytes(), &[bump]]],
    )
}

/// Founder only. Accounts: [beneficiary (signer), config (w), founder vault (w),
/// founder authority, destination (w, owned by the beneficiary), token].
/// Transfers vested(now) - founder_claimed. No admin path exists.
fn release_vested(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let iter = &mut accounts.iter();
    let beneficiary = next_account_info(iter)?;
    let config_account = next_account_info(iter)?;
    let founder_vault = next_account_info(iter)?;
    let founder_authority = next_account_info(iter)?;
    let destination = next_account_info(iter)?;
    let token = next_account_info(iter)?;
    require_signer(beneficiary)?;
    require_key(token, &SPL_TOKEN_ID)?;
    let mut config = load_config(program_id, config_account)?;
    if config.founder_beneficiary != *beneficiary.key {
        return Err(LiteError::NotBeneficiary.into());
    }
    require_key(founder_vault, &config.founder_vault)?;
    let fauth = Pubkey::create_program_address(&[FOUNDER_AUTHORITY_SEED, config.mint.as_ref(), &[config.founder_authority_bump]], program_id)
        .map_err(|_| ProgramError::from(LiteError::BadPda))?;
    require_key(founder_authority, &fauth)?;
    let (dest_mint, dest_owner, _) = read_token_account(destination)?;
    if dest_mint != config.mint || dest_owner != *beneficiary.key {
        return Err(LiteError::BadAccount.into());
    }
    let vested_now = vested(config.genesis_ts, config.epoch_seconds, now()?);
    let amount = vested_now.checked_sub(config.founder_claimed).ok_or(LiteError::Overflow)?;
    if amount == 0 {
        return Err(LiteError::NothingVested.into());
    }
    config.founder_claimed = vested_now;
    config.pack(&mut config_account.try_borrow_mut_data()?);
    msg!("ARES:VestedReleased amount={} founder_claimed={}", amount, vested_now);
    invoke_signed(
        &token_ix(
            amount_ix(3, amount),
            vec![AccountMeta::new(*founder_vault.key, false), AccountMeta::new(*destination.key, false), AccountMeta::new_readonly(fauth, true)],
        ),
        &[founder_vault.clone(), destination.clone(), founder_authority.clone(), token.clone()],
        &[&[FOUNDER_AUTHORITY_SEED, config.mint.as_ref(), &[config.founder_authority_bump]]],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_800_000_000;

    #[test]
    fn genesis_constants_are_exact() {
        assert_eq!(MAX_SUPPLY, 100_000_000_000_000);
        assert_eq!(FOUNDER_ALLOCATION + MINING_ALLOCATION, MAX_SUPPLY);
        assert_eq!(FOUNDER_ALLOCATION * 10, MAX_SUPPLY);
        assert_eq!(YEAR_EPOCHS * DAY, 31_536_000);
        for (prod, frac) in [(COMMIT_END, (5, 6)), (REVEAL_END, (11, 12)), (CLOSE_GRACE, (1, 4)), (PUBLISH_WINDOW, (2, 1)), (CLAIM_DELAY, (3, 1))] {
            assert_eq!(prod * frac.1, DAY * frac.0);
            assert_eq!(scaled(prod, DAY), prod);
            assert_eq!(scaled(prod, 24) * DAY, prod * 24, "exact at 24 s epochs");
        }
    }

    #[test]
    fn g12_cap_thresholds_and_half_life() {
        assert_eq!(epoch_cap(2_634), 1);
        assert_eq!(epoch_cap(2_633), 0);
        assert_eq!(epoch_cap(MINING_ALLOCATION), 34_176_150_000);
        assert_eq!(epoch_cap(u64::MAX), (u64::MAX as u128 * 379_735 / 1_000_000_000) as u64);
        let mut a = MINING_ALLOCATION;
        for _ in 0..1_824 {
            a -= epoch_cap(a);
        }
        assert!(a > MINING_ALLOCATION / 2);
        a -= epoch_cap(a);
        assert!(a <= MINING_ALLOCATION / 2);
        assert_eq!(a, 44_999_963_593_560);
    }

    #[test]
    fn g11_vesting_points() {
        let y = 31_536_000;
        let cliff = T0 + y;
        let end = T0 + 5 * y;
        let pts = [
            (T0, 0),
            (cliff - 1, 0),
            (cliff, 0),
            (cliff + 1, 79_274),
            (T0 + y + y / 2, 1_250_000_000_000),
            (T0 + 2 * y, 2_500_000_000_000),
            (T0 + 3 * y, 5_000_000_000_000),
            (T0 + 4 * y, 7_500_000_000_000),
            (end - 1, 9_999_999_920_725),
            (end, 10_000_000_000_000),
            (end + y, 10_000_000_000_000),
            (i64::MAX, 10_000_000_000_000),
        ];
        for (t, v) in pts {
            assert_eq!(vested(T0, DAY, t), v, "t={t}");
        }
    }

    #[test]
    fn bounty_rounding() {
        assert_eq!(bounty_split(1_000_000_000), (50_000_000, 950_000_000));
        assert_eq!(bounty_split(1_000_000_019), (50_000_000, 950_000_019));
        assert_eq!(bounty_split(MIN_BOUNTY), (50_000, 950_000));
        let (f, e) = bounty_split(u64::MAX);
        assert_eq!(f + e, u64::MAX);
    }

    #[test]
    fn close_window_boundaries() {
        let (o, e) = close_window(T0, DAY, 0).unwrap();
        assert_eq!((o, e), (T0 + 79_200, T0 + 86_400 + 21_600));
        let (o, e) = close_window(T0, DAY, 5).unwrap();
        assert_eq!((o, e), (T0 + 5 * DAY + 79_200, T0 + 6 * DAY + 21_600));
        assert!(close_window(T0, DAY, u64::MAX).is_none());
        assert!(epoch_start(T0, DAY, i64::MAX as u64).is_none());
    }

    #[test]
    fn production_build_only_accepts_24h_epochs() {
        assert!(epoch_seconds_allowed(DAY));
        #[cfg(not(feature = "devnet-time-scale"))]
        {
            assert!(!epoch_seconds_allowed(24));
            assert!(!epoch_seconds_allowed(DAY - 12));
        }
    }

    #[test]
    fn available_underflow_is_none() {
        assert_eq!(available(10, 3, 7), Some(0));
        assert_eq!(available(10, 3, 8), None);
        assert_eq!(available(10, 11, 0), None);
    }

    #[test]
    fn merkle_matches_python_vector() {
        let claimant = Pubkey::new_from_array([7; 32]);
        let leaf = leaf_hash(0, &claimant, 1_000);
        let hex: String = leaf.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, include_str!("../tests/vector_leaf.txt").trim());
    }

    #[test]
    fn layouts_fit() {
        assert!(255 <= CONFIG_LEN);
        assert!(292 <= EPOCH_LEN);
        let c = Config {
            version: 4,
            admin: Pubkey::new_from_array([1; 32]),
            mint: Pubkey::new_from_array([2; 32]),
            vault: Pubkey::new_from_array([3; 32]),
            founder_vault: Pubkey::new_from_array([4; 32]),
            founder_beneficiary: Pubkey::new_from_array([5; 32]),
            genesis_ts: T0,
            epoch_seconds: DAY,
            pending_reserved: 6,
            outstanding_unclaimed: 7,
            last_closed_epoch: NONE_EPOCH,
            recycled_total: 8,
            mining_claimed_total: 9,
            founder_claimed: 10,
            bounty_escrowed: 11,
            bounties_awarded: 12,
            mining_awarded_total: 13,
            bump: 1,
            vault_bump: 2,
            vault_authority_bump: 3,
            founder_vault_bump: 4,
            founder_authority_bump: 5,
        };
        let mut buf = vec![0u8; CONFIG_LEN];
        c.pack(&mut buf);
        assert_eq!(Config::unpack(&buf).unwrap(), c);
        let ep = Epoch {
            status: ST_CORRECTED,
            epoch: 9,
            manifest_hash: [1; 32],
            log_head: [2; 32],
            close_slot: 3,
            cap: 4,
            root: [5; 32],
            total: 6,
            digest: [7; 32],
            claimed: 8,
            deadline: 9,
            claim_open_ts: 10,
            orig_root: [11; 32],
            orig_total: 12,
            orig_digest: [13; 32],
            corrections: 1,
            bump: 14,
            closed_ts: 15,
            published_ts: 16,
            corrected_ts: 17,
            withdrawn_ts: 18,
        };
        let mut buf = vec![0u8; EPOCH_LEN];
        ep.pack(&mut buf);
        assert_eq!(Epoch::unpack(&buf).unwrap(), ep);
    }

    #[test]
    fn unpack_rejects_trailing_and_unknown() {
        let mut init = vec![0u8];
        init.extend_from_slice(&[9; 32]);
        init.extend_from_slice(&86_400u64.to_le_bytes());
        assert!(LiteInstruction::unpack(&init).is_ok());
        init.push(0);
        assert!(LiteInstruction::unpack(&init).is_err());
        assert!(LiteInstruction::unpack(&[0]).is_err());
        assert!(LiteInstruction::unpack(&[13]).is_err());
        assert!(LiteInstruction::unpack(&[11]).is_ok());
        assert!(LiteInstruction::unpack(&[12]).is_ok());
        let mut long = vec![8u8];
        long.extend_from_slice(&1u64.to_le_bytes());
        long.extend_from_slice(&1u64.to_le_bytes());
        long.push((MAX_PROOF_LEN + 1) as u8);
        assert!(LiteInstruction::unpack(&long).is_err());
    }
}
