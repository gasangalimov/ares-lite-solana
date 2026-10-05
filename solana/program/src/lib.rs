//! ARES Lite rewards program v2 — FIXED SUPPLY.
//!
//! * `Initialize` (atomic genesis): mints EXACTLY 1,000,000,000 ARES (6 dp):
//!   100,000,000 to the founder token account, 900,000,000 to the protocol
//!   reward vault (a token account owned by a PDA), then revokes the mint
//!   authority (SetAuthority -> None). The mint must have no freeze
//!   authority. After this instruction nobody can ever mint again; supply can
//!   only stay equal or decrease through burns. The genesis timestamp is
//!   recorded immutably.
//! * Reward release ("distribution halving", NOT mint halving): the vault may
//!   only have committed `unlocked(now)` ARES in total, where era n (5 years
//!   each, exactly ERA_SECONDS) releases ERA0_BUDGET >> n linearly. No
//!   instruction can change the schedule.
//! * `CreateSeason` / `PublishRewardCommitment` / `Claim`: per-season Merkle
//!   root (published once, immutable), claims TRANSFER existing ARES from the
//!   vault; `committed <= unlocked(now)` is enforced at publication.
//! * `FundBounty` / `AwardBounty`: utility burn. Funding a bounty burns
//!   exactly BOUNTY_BURN_BPS of it with a real SPL Burn (supply decreases) and
//!   escrows the rest for the solver.
//! * `SetAdmin`: hand the admin role to a multisig.
//!
//! There is no instruction that mints, changes the schedule, replaces a root,
//! freezes accounts, or moves/burns tokens the program does not own.

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

pub const PROGRAM_VERSION: u8 = 3;
pub const SPL_TOKEN_ID: Pubkey = solana_program::pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

// ------------------------------------------------------------ token economics

pub const DECIMALS: u8 = 6;
pub const UNIT: u64 = 1_000_000;
pub const TOTAL_SUPPLY: u64 = 1_000_000_000 * UNIT;
pub const FOUNDER_ALLOCATION: u64 = 100_000_000 * UNIT;
pub const REWARD_RESERVE: u64 = 900_000_000 * UNIT;
/// Era 0 release budget; era n releases ERA0_BUDGET >> n.
pub const ERA0_BUDGET: u64 = 450_000_000 * UNIT;
/// Exactly five Julian years: 5 * 365.25 * 86_400 seconds.
pub const ERA_SECONDS: i64 = 157_788_000;
/// A mining/competition epoch can never pay more than what the schedule
/// unlocked since the previous settlement, looking back at most this long.
/// Separates the 5-year HALVING ERA (release speed) from the MINING EPOCH.
pub const MAX_EPOCH_SECONDS: i64 = 7 * 86_400;
/// Upper bound for the claim delay (public verification window) set at genesis.
pub const MAX_CLAIM_DELAY_SECONDS: i64 = 30 * 86_400;
/// Bounty burn: 10.00% (basis points), fixed in code.
pub const BOUNTY_BURN_BPS: u64 = 1_000;

pub const CONFIG_SEED: &[u8] = b"config";
pub const VAULT_SEED: &[u8] = b"vault";
pub const VAULT_AUTHORITY_SEED: &[u8] = b"vault_authority";
pub const SEASON_SEED: &[u8] = b"season";
pub const CLAIM_SEED: &[u8] = b"claim";
pub const BOUNTY_SEED: &[u8] = b"bounty";
pub const ESCROW_SEED: &[u8] = b"escrow";

pub const CLAIM_LEAF_TAG: &[u8] = b"ARES-LITE/CLAIM-LEAF/v0";
pub const CLAIM_NODE_TAG: &[u8] = b"ARES-LITE/CLAIM-NODE/v0";
pub const MAX_PROOF_LEN: usize = 32;

const CONFIG_TAG: u8 = 11;
const SEASON_TAG: u8 = 12;
const RECEIPT_TAG: u8 = 13;
const BOUNTY_TAG: u8 = 14;
pub const CONFIG_LEN: usize = 1 + 1 + 32 * 4 + 8 * 6 + 3 + 16;
pub const SEASON_LEN: usize = 187;
pub const RECEIPT_LEN: usize = 50;
pub const BOUNTY_LEN: usize = 1 + 1 + 8 + 32 + 8 + 8 + 1;
const MINT_LEN: usize = 82;
const TOKEN_ACCOUNT_LEN: u64 = 165;

pub const STATUS_OPEN: u8 = 0;
pub const STATUS_COMMITTED: u8 = 1;
pub const STATUS_CLOSED: u8 = 2;
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
    SeasonExists = 8,
    RootAlreadyPublished = 9,
    RootNotPublished = 10,
    InvalidProof = 11,
    AlreadyClaimed = 12,
    BadAccount = 13,
    ZeroAmount = 14,
    GenesisInvariant = 15,
    ScheduleExceeded = 16,
    BountyClosed = 17,
    WrongPhase = 18,
    LogHeadMismatch = 19,
    ClaimWindowNotOpen = 20,
    EpochCapExceeded = 21,
}

impl From<LiteError> for ProgramError {
    fn from(error: LiteError) -> Self {
        ProgramError::Custom(error as u32)
    }
}

// ------------------------------------------------------------ release schedule

/// Maximum ARES (base units) the protocol may have released at `elapsed`
/// seconds after genesis. Era n = [n*ERA, (n+1)*ERA) releases
/// (ERA0_BUDGET >> n) linearly. Monotone, never exceeds REWARD_RESERVE.
pub fn unlocked(elapsed: i64) -> u64 {
    if elapsed <= 0 {
        return 0;
    }
    let era = (elapsed / ERA_SECONDS) as u64;
    let within = (elapsed % ERA_SECONDS) as u128;
    let mut total: u128 = 0;
    let full = era.min(64);
    let mut k = 0;
    while k < full {
        total += (ERA0_BUDGET >> k) as u128;
        k += 1;
    }
    if era < 64 {
        total += (ERA0_BUDGET >> era) as u128 * within / ERA_SECONDS as u128;
    }
    total as u64
}

/// Current era index and its full budget.
pub fn era_at(elapsed: i64) -> (u64, u64) {
    let era = if elapsed <= 0 { 0 } else { (elapsed / ERA_SECONDS) as u64 };
    (era, if era < 64 { ERA0_BUDGET >> era } else { 0 })
}

/// epoch_reward_cap = unlocked(now) - unlocked(max(last_settlement, now - MAX_EPOCH)).
/// A single epoch winner can never receive more than the ARES newly unlocked
/// since the previous settlement, and never more than MAX_EPOCH_SECONDS worth.
pub fn epoch_reward_cap(genesis_ts: i64, last_settlement_ts: i64, now_ts: i64) -> u64 {
    let start = last_settlement_ts.max(now_ts - MAX_EPOCH_SECONDS).max(genesis_ts);
    if now_ts <= start {
        return 0;
    }
    unlocked(now_ts - genesis_ts) - unlocked(start - genesis_ts)
}

pub fn bounty_burn(amount: u64) -> u64 {
    ((amount as u128 * BOUNTY_BURN_BPS as u128) / 10_000) as u64
}

// --------------------------------------------------------------- instructions

pub enum LiteInstruction {
    Initialize { claim_delay_seconds: u64 },
    CreateSeason { season_id: u64, manifest_hash: [u8; 32] },
    PublishRewardCommitment { reward_root: [u8; 32], reward_total: u64, log_head: [u8; 32], results_digest: [u8; 32] },
    Claim { amount: u64, proof: Vec<[u8; 32]> },
    SetAdmin { new_admin: Pubkey },
    FundBounty { bounty_id: u64, amount: u64 },
    AwardBounty,
    CloseSeason { log_head: [u8; 32], close_slot: u64 },
    CancelRoot,
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
        let tag = take(&mut data, 1)?[0];
        let instruction = match tag {
            0 => Self::Initialize { claim_delay_seconds: take_u64(&mut data)? },
            1 => Self::CreateSeason { season_id: take_u64(&mut data)?, manifest_hash: take_32(&mut data)? },
            2 => Self::PublishRewardCommitment {
                reward_root: take_32(&mut data)?,
                reward_total: take_u64(&mut data)?,
                log_head: take_32(&mut data)?,
                results_digest: take_32(&mut data)?,
            },
            3 => {
                let amount = take_u64(&mut data)?;
                let count = take(&mut data, 1)?[0] as usize;
                if count > MAX_PROOF_LEN {
                    return Err(LiteError::InvalidInstruction.into());
                }
                let mut proof = Vec::with_capacity(count);
                for _ in 0..count {
                    proof.push(take_32(&mut data)?);
                }
                Self::Claim { amount, proof }
            }
            4 => Self::SetAdmin { new_admin: Pubkey::new_from_array(take_32(&mut data)?) },
            5 => Self::FundBounty { bounty_id: take_u64(&mut data)?, amount: take_u64(&mut data)? },
            6 => Self::AwardBounty,
            7 => Self::CloseSeason { log_head: take_32(&mut data)?, close_slot: take_u64(&mut data)? },
            8 => Self::CancelRoot,
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
pub fn season_address(program_id: &Pubkey, config: &Pubkey, season_id: u64) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[SEASON_SEED, config.as_ref(), &season_id.to_le_bytes()], program_id)
}
pub fn claim_address(program_id: &Pubkey, season: &Pubkey, claimant: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[CLAIM_SEED, season.as_ref(), claimant.as_ref()], program_id)
}
pub fn bounty_address(program_id: &Pubkey, config: &Pubkey, bounty_id: u64) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[BOUNTY_SEED, config.as_ref(), &bounty_id.to_le_bytes()], program_id)
}
pub fn escrow_address(program_id: &Pubkey, bounty: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[ESCROW_SEED, bounty.as_ref()], program_id)
}

// --------------------------------------------------------------------- merkle

pub fn leaf_hash(season_id: u64, claimant: &Pubkey, amount: u64) -> [u8; 32] {
    hashv(&[&[0u8], CLAIM_LEAF_TAG, &season_id.to_le_bytes(), claimant.as_ref(), &amount.to_le_bytes()])
        .to_bytes()
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub version: u8,
    pub admin: Pubkey,
    pub mint: Pubkey,
    pub vault: Pubkey,
    pub founder: Pubkey,
    pub genesis_ts: i64,
    pub committed: u64,
    pub distributed: u64,
    pub bounty_burned: u64,
    pub bounty_escrowed: u64,
    pub bounties_awarded: u64,
    pub last_settlement_ts: i64,
    pub claim_delay: i64,
    pub bump: u8,
    pub vault_bump: u8,
    pub vault_authority_bump: u8,
}

impl Config {
    pub fn unpack(data: &[u8]) -> Result<Self, ProgramError> {
        if data.len() != CONFIG_LEN || data[0] != CONFIG_TAG {
            return Err(LiteError::BadAccount.into());
        }
        let key = |at: usize| Pubkey::new_from_array(data[at..at + 32].try_into().unwrap());
        let num = |at: usize| u64::from_le_bytes(data[at..at + 8].try_into().unwrap());
        Ok(Self {
            version: data[1],
            admin: key(2),
            mint: key(34),
            vault: key(66),
            founder: key(98),
            genesis_ts: num(130) as i64,
            committed: num(138),
            distributed: num(146),
            bounty_burned: num(154),
            bounty_escrowed: num(162),
            bounties_awarded: num(170),
            bump: data[178],
            vault_bump: data[179],
            vault_authority_bump: data[180],
            last_settlement_ts: num(181) as i64,
            claim_delay: num(189) as i64,
        })
    }

    fn pack(&self, data: &mut [u8]) {
        data[0] = CONFIG_TAG;
        data[1] = self.version;
        data[2..34].copy_from_slice(self.admin.as_ref());
        data[34..66].copy_from_slice(self.mint.as_ref());
        data[66..98].copy_from_slice(self.vault.as_ref());
        data[98..130].copy_from_slice(self.founder.as_ref());
        data[130..138].copy_from_slice(&(self.genesis_ts as u64).to_le_bytes());
        data[138..146].copy_from_slice(&self.committed.to_le_bytes());
        data[146..154].copy_from_slice(&self.distributed.to_le_bytes());
        data[154..162].copy_from_slice(&self.bounty_burned.to_le_bytes());
        data[162..170].copy_from_slice(&self.bounty_escrowed.to_le_bytes());
        data[170..178].copy_from_slice(&self.bounties_awarded.to_le_bytes());
        data[178] = self.bump;
        data[179] = self.vault_bump;
        data[180] = self.vault_authority_bump;
        data[181..189].copy_from_slice(&(self.last_settlement_ts as u64).to_le_bytes());
        data[189..197].copy_from_slice(&(self.claim_delay as u64).to_le_bytes());
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Season {
    pub status: u8,
    pub season_id: u64,
    pub manifest_hash: [u8; 32],
    pub reward_root: [u8; 32],
    pub reward_total: u64,
    pub log_head: [u8; 32],
    pub claimed: u64,
    pub bump: u8,
    pub close_slot: u64,
    pub results_digest: [u8; 32],
    pub claim_open_ts: i64,
    pub prev_settlement_ts: i64,
    /// epoch_reward_cap fixed at close time: the deterministic epoch reward.
    pub close_cap: u64,
}

impl Season {
    pub fn unpack(data: &[u8]) -> Result<Self, ProgramError> {
        if data.len() != SEASON_LEN || data[0] != SEASON_TAG {
            return Err(LiteError::BadAccount.into());
        }
        Ok(Self {
            status: data[1],
            season_id: u64::from_le_bytes(data[2..10].try_into().unwrap()),
            manifest_hash: data[10..42].try_into().unwrap(),
            reward_root: data[50..82].try_into().unwrap(),
            reward_total: u64::from_le_bytes(data[82..90].try_into().unwrap()),
            log_head: data[90..122].try_into().unwrap(),
            claimed: u64::from_le_bytes(data[122..130].try_into().unwrap()),
            bump: data[130],
            close_slot: u64::from_le_bytes(data[42..50].try_into().unwrap()),
            results_digest: data[131..163].try_into().unwrap(),
            claim_open_ts: u64::from_le_bytes(data[163..171].try_into().unwrap()) as i64,
            prev_settlement_ts: u64::from_le_bytes(data[171..179].try_into().unwrap()) as i64,
            close_cap: u64::from_le_bytes(data[179..187].try_into().unwrap()),
        })
    }

    fn pack(&self, data: &mut [u8]) {
        data[0] = SEASON_TAG;
        data[1] = self.status;
        data[2..10].copy_from_slice(&self.season_id.to_le_bytes());
        data[10..42].copy_from_slice(&self.manifest_hash);
        data[42..50].copy_from_slice(&self.close_slot.to_le_bytes());
        data[50..82].copy_from_slice(&self.reward_root);
        data[82..90].copy_from_slice(&self.reward_total.to_le_bytes());
        data[90..122].copy_from_slice(&self.log_head);
        data[122..130].copy_from_slice(&self.claimed.to_le_bytes());
        data[130] = self.bump;
        data[131..163].copy_from_slice(&self.results_digest);
        data[163..171].copy_from_slice(&(self.claim_open_ts as u64).to_le_bytes());
        data[171..179].copy_from_slice(&(self.prev_settlement_ts as u64).to_le_bytes());
        data[179..187].copy_from_slice(&self.close_cap.to_le_bytes());
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
    Ok((
        Pubkey::new_from_array(data[0..32].try_into().unwrap()),
        Pubkey::new_from_array(data[32..64].try_into().unwrap()),
        u64::from_le_bytes(data[64..72].try_into().unwrap()),
    ))
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
            invoke(
                &system_instruction::transfer(payer.key, target.key, top_up),
                &[payer.clone(), target.clone(), system.clone()],
            )?;
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

fn load_season(program_id: &Pubkey, config_key: &Pubkey, account: &AccountInfo) -> Result<Season, ProgramError> {
    if account.owner != program_id {
        return Err(LiteError::BadAccount.into());
    }
    let season = Season::unpack(&account.try_borrow_data()?)?;
    let expected = Pubkey::create_program_address(
        &[SEASON_SEED, config_key.as_ref(), &season.season_id.to_le_bytes(), &[season.bump]],
        program_id,
    )
    .map_err(|_| ProgramError::from(LiteError::BadPda))?;
    require_key(account, &expected)?;
    Ok(season)
}

fn now() -> Result<i64, ProgramError> {
    Ok(Clock::get()?.unix_timestamp)
}

// ----------------------------------------------------------------- processors

pub fn process_instruction(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    match LiteInstruction::unpack(data)? {
        LiteInstruction::Initialize { claim_delay_seconds } => initialize(program_id, accounts, claim_delay_seconds),
        LiteInstruction::CreateSeason { season_id, manifest_hash } => create_season(program_id, accounts, season_id, manifest_hash),
        LiteInstruction::PublishRewardCommitment { reward_root, reward_total, log_head, results_digest } => {
            publish(program_id, accounts, reward_root, reward_total, log_head, results_digest)
        }
        LiteInstruction::Claim { amount, proof } => claim(program_id, accounts, amount, &proof),
        LiteInstruction::SetAdmin { new_admin } => set_admin(program_id, accounts, new_admin),
        LiteInstruction::FundBounty { bounty_id, amount } => fund_bounty(program_id, accounts, bounty_id, amount),
        LiteInstruction::AwardBounty => award_bounty(program_id, accounts),
        LiteInstruction::CloseSeason { log_head, close_slot } => close_season(program_id, accounts, log_head, close_slot),
        LiteInstruction::CancelRoot => cancel_root(program_id, accounts),
    }
}

/// Atomic genesis.
/// Accounts: [deployer (signer, w; current mint authority, becomes admin),
/// config (w), mint (w), vault (w, PDA token account), vault authority PDA,
/// founder token account (w), system, token].
fn initialize(program_id: &Pubkey, accounts: &[AccountInfo], claim_delay_seconds: u64) -> ProgramResult {
    let iter = &mut accounts.iter();
    let deployer = next_account_info(iter)?;
    let config_account = next_account_info(iter)?;
    let mint = next_account_info(iter)?;
    let vault = next_account_info(iter)?;
    let vault_authority = next_account_info(iter)?;
    let founder_account = next_account_info(iter)?;
    let system = next_account_info(iter)?;
    let token = next_account_info(iter)?;
    require_signer(deployer)?;
    require_key(system, &system_program::ID)?;
    require_key(token, &SPL_TOKEN_ID)?;
    if claim_delay_seconds > MAX_CLAIM_DELAY_SECONDS as u64 {
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
    require_key(vault, &vault_key)?;
    require_key(vault_authority, &authority_key)?;
    let (founder_mint, founder_owner, _) = read_token_account(founder_account)?;
    if founder_mint != *mint.key || founder_account.key == vault.key {
        return Err(LiteError::BadAccount.into());
    }

    create_pda(deployer, config_account, system, program_id, CONFIG_LEN, &[CONFIG_SEED, mint.key.as_ref(), &[bump]])?;
    // Reward vault: SPL token account at a PDA address, owned by the vault authority PDA.
    create_pda(deployer, vault, system, &SPL_TOKEN_ID, TOKEN_ACCOUNT_LEN as usize, &[VAULT_SEED, mint.key.as_ref(), &[vault_bump]])?;
    let mut init = vec![18u8];
    init.extend_from_slice(authority_key.as_ref());
    invoke(
        &token_ix(init, vec![AccountMeta::new(vault_key, false), AccountMeta::new_readonly(*mint.key, false)]),
        &[vault.clone(), mint.clone(), token.clone()],
    )?;
    // Exactly 900M to the vault, exactly 100M to the founder.
    let mint_meta = |to: &Pubkey| vec![AccountMeta::new(*mint.key, false), AccountMeta::new(*to, false), AccountMeta::new_readonly(*deployer.key, true)];
    invoke(&token_ix(amount_ix(7, REWARD_RESERVE), mint_meta(&vault_key)), &[mint.clone(), vault.clone(), deployer.clone(), token.clone()])?;
    invoke(
        &token_ix(amount_ix(7, FOUNDER_ALLOCATION), mint_meta(founder_account.key)),
        &[mint.clone(), founder_account.clone(), deployer.clone(), token.clone()],
    )?;
    // Revoke the mint authority forever: SetAuthority(MintTokens, None).
    invoke(
        &token_ix(vec![6u8, 0u8, 0u8], vec![AccountMeta::new(*mint.key, false), AccountMeta::new_readonly(*deployer.key, true)]),
        &[mint.clone(), deployer.clone(), token.clone()],
    )?;
    // Post-conditions, checked on-chain in the same transaction.
    let after = read_mint(mint)?;
    let (_, _, vault_balance) = read_token_account(vault)?;
    if after.supply != TOTAL_SUPPLY
        || after.mint_authority.is_some()
        || after.freeze_authority.is_some()
        || vault_balance != REWARD_RESERVE
        || FOUNDER_ALLOCATION + REWARD_RESERVE != TOTAL_SUPPLY
    {
        return Err(LiteError::GenesisInvariant.into());
    }

    Config {
        version: PROGRAM_VERSION,
        admin: *deployer.key,
        mint: *mint.key,
        vault: vault_key,
        founder: founder_owner,
        genesis_ts: now()?,
        last_settlement_ts: now()?,
        claim_delay: claim_delay_seconds as i64,
        committed: 0,
        distributed: 0,
        bounty_burned: 0,
        bounty_escrowed: 0,
        bounties_awarded: 0,
        bump,
        vault_bump,
        vault_authority_bump: authority_bump,
    }
    .pack(&mut config_account.try_borrow_mut_data()?);
    msg!("ARES genesis: supply {} fixed; mint authority revoked", TOTAL_SUPPLY);
    Ok(())
}

/// Accounts: [admin (signer, w), config, season (w), system].
fn create_season(program_id: &Pubkey, accounts: &[AccountInfo], season_id: u64, manifest_hash: [u8; 32]) -> ProgramResult {
    let iter = &mut accounts.iter();
    let admin = next_account_info(iter)?;
    let config_account = next_account_info(iter)?;
    let season_account = next_account_info(iter)?;
    let system = next_account_info(iter)?;
    require_signer(admin)?;
    require_key(system, &system_program::ID)?;
    let config = load_config(program_id, config_account)?;
    if config.admin != *admin.key {
        return Err(LiteError::NotAdmin.into());
    }
    let (season_key, bump) = season_address(program_id, config_account.key, season_id);
    require_key(season_account, &season_key)?;
    if season_account.owner == program_id || !season_account.data_is_empty() {
        return Err(LiteError::SeasonExists.into());
    }
    create_pda(admin, season_account, system, program_id, SEASON_LEN, &[SEASON_SEED, config_account.key.as_ref(), &season_id.to_le_bytes(), &[bump]])?;
    Season {
        status: STATUS_OPEN,
        season_id,
        manifest_hash,
        reward_root: [0; 32],
        reward_total: 0,
        log_head: [0; 32],
        claimed: 0,
        bump,
        close_slot: 0,
        results_digest: [0; 32],
        claim_open_ts: 0,
        prev_settlement_ts: 0,
        close_cap: 0,
    }
        .pack(&mut season_account.try_borrow_mut_data()?);
    Ok(())
}

/// Accounts: [admin (signer), config (w), season (w)].
/// Pins the closed receipt-log head BEFORE results exist: after this, the set
/// of submissions that can be scored is fixed for everybody.
fn close_season(program_id: &Pubkey, accounts: &[AccountInfo], log_head: [u8; 32], close_slot: u64) -> ProgramResult {
    let iter = &mut accounts.iter();
    let admin = next_account_info(iter)?;
    let config_account = next_account_info(iter)?;
    let season_account = next_account_info(iter)?;
    require_signer(admin)?;
    let config = load_config(program_id, config_account)?;
    if config.admin != *admin.key {
        return Err(LiteError::NotAdmin.into());
    }
    let mut season = load_season(program_id, config_account.key, season_account)?;
    if season.status != STATUS_OPEN {
        return Err(LiteError::WrongPhase.into());
    }
    season.log_head = log_head;
    season.close_slot = close_slot;
    season.close_cap = epoch_reward_cap(config.genesis_ts, config.last_settlement_ts, now()?);
    season.status = STATUS_CLOSED;
    season.pack(&mut season_account.try_borrow_mut_data()?);
    Ok(())
}

/// Accounts: [admin (signer), config (w), season (w)].
/// The root must be for the log head pinned at close; the amount is bounded
/// by the per-epoch cap; claims open only after the public verification
/// window (claim_delay) during which anyone can recompute the result.
fn publish(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    reward_root: [u8; 32],
    reward_total: u64,
    log_head: [u8; 32],
    results_digest: [u8; 32],
) -> ProgramResult {
    let iter = &mut accounts.iter();
    let admin = next_account_info(iter)?;
    let config_account = next_account_info(iter)?;
    let season_account = next_account_info(iter)?;
    require_signer(admin)?;
    let mut config = load_config(program_id, config_account)?;
    if config.admin != *admin.key {
        return Err(LiteError::NotAdmin.into());
    }
    let mut season = load_season(program_id, config_account.key, season_account)?;
    if season.status == STATUS_COMMITTED {
        return Err(LiteError::RootAlreadyPublished.into());
    }
    if season.status != STATUS_CLOSED {
        return Err(LiteError::WrongPhase.into());
    }
    if season.log_head != log_head {
        return Err(LiteError::LogHeadMismatch.into());
    }
    let now = now()?;
    if reward_total > season.close_cap || reward_total > epoch_reward_cap(config.genesis_ts, config.last_settlement_ts, now) {
        return Err(LiteError::EpochCapExceeded.into());
    }
    let committed = config.committed.checked_add(reward_total).ok_or(LiteError::Overflow)?;
    if committed > unlocked(now - config.genesis_ts) {
        return Err(LiteError::ScheduleExceeded.into());
    }
    season.reward_root = reward_root;
    season.reward_total = reward_total;
    season.results_digest = results_digest;
    season.prev_settlement_ts = config.last_settlement_ts;
    season.claim_open_ts = now.checked_add(config.claim_delay).ok_or(LiteError::Overflow)?;
    season.status = STATUS_COMMITTED;
    season.pack(&mut season_account.try_borrow_mut_data()?);
    config.committed = committed;
    config.last_settlement_ts = now;
    config.pack(&mut config_account.try_borrow_mut_data()?);
    Ok(())
}

/// Accounts: [admin (signer), config (w), season (w)].
/// Withdraw a published root during the verification window (e.g. when
/// independent recomputation disagrees). Only the latest settlement, only
/// before any claim; the epoch budget is restored, the season returns to
/// CLOSED (same pinned log head) and must be republished.
fn cancel_root(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let iter = &mut accounts.iter();
    let admin = next_account_info(iter)?;
    let config_account = next_account_info(iter)?;
    let season_account = next_account_info(iter)?;
    require_signer(admin)?;
    let mut config = load_config(program_id, config_account)?;
    if config.admin != *admin.key {
        return Err(LiteError::NotAdmin.into());
    }
    let mut season = load_season(program_id, config_account.key, season_account)?;
    let published_at = season.claim_open_ts - config.claim_delay;
    if season.status != STATUS_COMMITTED || now()? >= season.claim_open_ts || season.claimed != 0
        || config.last_settlement_ts != published_at
    {
        return Err(LiteError::WrongPhase.into());
    }
    config.committed = config.committed.checked_sub(season.reward_total).ok_or(LiteError::Overflow)?;
    config.last_settlement_ts = season.prev_settlement_ts;
    season.status = STATUS_CLOSED;
    season.reward_root = [0; 32];
    season.reward_total = 0;
    season.results_digest = [0; 32];
    season.claim_open_ts = 0;
    season.pack(&mut season_account.try_borrow_mut_data()?);
    config.pack(&mut config_account.try_borrow_mut_data()?);
    Ok(())
}

/// Accounts: [claimant (signer, w), config (w), season (w), receipt (w),
/// vault (w), destination token account (w), vault authority PDA, token, system].
fn claim(program_id: &Pubkey, accounts: &[AccountInfo], amount: u64, proof: &[[u8; 32]]) -> ProgramResult {
    let iter = &mut accounts.iter();
    let claimant = next_account_info(iter)?;
    let config_account = next_account_info(iter)?;
    let season_account = next_account_info(iter)?;
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
    let authority_key = Pubkey::create_program_address(&[VAULT_AUTHORITY_SEED, config.mint.as_ref(), &[config.vault_authority_bump]], program_id)
        .map_err(|_| ProgramError::from(LiteError::BadPda))?;
    require_key(vault_authority, &authority_key)?;
    let mut season = load_season(program_id, config_account.key, season_account)?;
    if season.status != STATUS_COMMITTED {
        return Err(LiteError::RootNotPublished.into());
    }
    if now()? < season.claim_open_ts {
        return Err(LiteError::ClaimWindowNotOpen.into());
    }
    if !verify_proof(&season.reward_root, leaf_hash(season.season_id, claimant.key, amount), proof) {
        return Err(LiteError::InvalidProof.into());
    }
    let (receipt_key, receipt_bump) = claim_address(program_id, season_account.key, claimant.key);
    require_key(receipt, &receipt_key)?;
    if receipt.owner == program_id || !receipt.data_is_empty() {
        return Err(LiteError::AlreadyClaimed.into());
    }
    let claimed = season.claimed.checked_add(amount).ok_or(LiteError::Overflow)?;
    if claimed > season.reward_total {
        return Err(LiteError::CapExceeded.into());
    }
    let distributed = config.distributed.checked_add(amount).ok_or(LiteError::Overflow)?;
    if distributed > config.committed || distributed > REWARD_RESERVE {
        return Err(LiteError::CapExceeded.into());
    }
    create_pda(claimant, receipt, system, program_id, RECEIPT_LEN, &[CLAIM_SEED, season_account.key.as_ref(), claimant.key.as_ref(), &[receipt_bump]])?;
    {
        let mut data = receipt.try_borrow_mut_data()?;
        data[0] = RECEIPT_TAG;
        data[1..9].copy_from_slice(&season.season_id.to_le_bytes());
        data[9..41].copy_from_slice(claimant.key.as_ref());
        data[41..49].copy_from_slice(&amount.to_le_bytes());
        data[49] = receipt_bump;
    }
    season.claimed = claimed;
    season.pack(&mut season_account.try_borrow_mut_data()?);
    config.distributed = distributed;
    config.pack(&mut config_account.try_borrow_mut_data()?);
    // Transfer EXISTING ARES from the vault (no minting exists anymore).
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
    require_signer(admin)?;
    let mut config = load_config(program_id, config_account)?;
    if config.admin != *admin.key {
        return Err(LiteError::NotAdmin.into());
    }
    config.admin = new_admin;
    config.pack(&mut config_account.try_borrow_mut_data()?);
    Ok(())
}

/// Utility burn. Accounts: [funder (signer, w), funder token account (w),
/// config (w), mint (w), bounty (w, PDA), escrow (w, PDA token account), system, token].
/// Burns exactly floor(amount * BOUNTY_BURN_BPS / 10000) from the funder's own
/// account (the funder signs) and escrows the remainder.
fn fund_bounty(program_id: &Pubkey, accounts: &[AccountInfo], bounty_id: u64, amount: u64) -> ProgramResult {
    let iter = &mut accounts.iter();
    let funder = next_account_info(iter)?;
    let funder_account = next_account_info(iter)?;
    let config_account = next_account_info(iter)?;
    let mint = next_account_info(iter)?;
    let bounty = next_account_info(iter)?;
    let escrow = next_account_info(iter)?;
    let system = next_account_info(iter)?;
    let token = next_account_info(iter)?;
    require_signer(funder)?;
    require_key(system, &system_program::ID)?;
    require_key(token, &SPL_TOKEN_ID)?;
    if amount == 0 {
        return Err(LiteError::ZeroAmount.into());
    }
    let mut config = load_config(program_id, config_account)?;
    require_key(mint, &config.mint)?;
    let (bounty_key, bounty_bump) = bounty_address(program_id, config_account.key, bounty_id);
    require_key(bounty, &bounty_key)?;
    if bounty.owner == program_id || !bounty.data_is_empty() {
        return Err(LiteError::AlreadyInitialized.into());
    }
    let (escrow_key, escrow_bump) = escrow_address(program_id, &bounty_key);
    require_key(escrow, &escrow_key)?;
    let burn = bounty_burn(amount);
    let reward = amount - burn;

    create_pda(funder, bounty, system, program_id, BOUNTY_LEN, &[BOUNTY_SEED, config_account.key.as_ref(), &bounty_id.to_le_bytes(), &[bounty_bump]])?;
    create_pda(funder, escrow, system, &SPL_TOKEN_ID, TOKEN_ACCOUNT_LEN as usize, &[ESCROW_SEED, bounty_key.as_ref(), &[escrow_bump]])?;
    let mut init = vec![18u8];
    init.extend_from_slice(bounty_key.as_ref());
    invoke(
        &token_ix(init, vec![AccountMeta::new(escrow_key, false), AccountMeta::new_readonly(*mint.key, false)]),
        &[escrow.clone(), mint.clone(), token.clone()],
    )?;
    // SPL Burn (supply decreases), signed by the funder for their own tokens.
    if burn > 0 {
        invoke(
            &token_ix(amount_ix(8, burn), vec![AccountMeta::new(*funder_account.key, false), AccountMeta::new(*mint.key, false), AccountMeta::new_readonly(*funder.key, true)]),
            &[funder_account.clone(), mint.clone(), funder.clone(), token.clone()],
        )?;
    }
    invoke(
        &token_ix(amount_ix(3, reward), vec![AccountMeta::new(*funder_account.key, false), AccountMeta::new(escrow_key, false), AccountMeta::new_readonly(*funder.key, true)]),
        &[funder_account.clone(), escrow.clone(), funder.clone(), token.clone()],
    )?;
    {
        let mut data = bounty.try_borrow_mut_data()?;
        data[0] = BOUNTY_TAG;
        data[1] = BOUNTY_FUNDED;
        data[2..10].copy_from_slice(&bounty_id.to_le_bytes());
        data[10..42].copy_from_slice(funder.key.as_ref());
        data[42..50].copy_from_slice(&reward.to_le_bytes());
        data[50..58].copy_from_slice(&burn.to_le_bytes());
        data[58] = bounty_bump;
    }
    config.bounty_burned = config.bounty_burned.checked_add(burn).ok_or(LiteError::Overflow)?;
    config.bounty_escrowed = config.bounty_escrowed.checked_add(reward).ok_or(LiteError::Overflow)?;
    config.pack(&mut config_account.try_borrow_mut_data()?);
    Ok(())
}

/// Accounts: [admin (signer), config (w), bounty (w), escrow (w), destination (w), token].
/// MVP: the admin/operator designates the solver (same trust as the reward
/// publisher); the full escrow goes out exactly once.
fn award_bounty(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let iter = &mut accounts.iter();
    let admin = next_account_info(iter)?;
    let config_account = next_account_info(iter)?;
    let bounty = next_account_info(iter)?;
    let escrow = next_account_info(iter)?;
    let destination = next_account_info(iter)?;
    let token = next_account_info(iter)?;
    require_signer(admin)?;
    require_key(token, &SPL_TOKEN_ID)?;
    let mut config = load_config(program_id, config_account)?;
    if config.admin != *admin.key {
        return Err(LiteError::NotAdmin.into());
    }
    if bounty.owner != program_id {
        return Err(LiteError::BadAccount.into());
    }
    let (status, bounty_id, reward, bump) = {
        let data = bounty.try_borrow_data()?;
        if data.len() != BOUNTY_LEN || data[0] != BOUNTY_TAG {
            return Err(LiteError::BadAccount.into());
        }
        (data[1], u64::from_le_bytes(data[2..10].try_into().unwrap()), u64::from_le_bytes(data[42..50].try_into().unwrap()), data[58])
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
        &token_ix(amount_ix(3, reward), vec![AccountMeta::new(*escrow.key, false), AccountMeta::new(*destination.key, false), AccountMeta::new_readonly(*bounty.key, true)]),
        &[escrow.clone(), destination.clone(), bounty.clone(), token.clone()],
        &[&[BOUNTY_SEED, config_account.key.as_ref(), &bounty_id.to_le_bytes(), &[bump]]],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genesis_constants_are_exact() {
        assert_eq!(TOTAL_SUPPLY, 1_000_000_000_000_000);
        assert_eq!(FOUNDER_ALLOCATION + REWARD_RESERVE, TOTAL_SUPPLY);
        assert_eq!(FOUNDER_ALLOCATION * 10, TOTAL_SUPPLY);
        assert_eq!(ERA0_BUDGET * 2, REWARD_RESERVE);
        assert_eq!(ERA_SECONDS, 5 * 36_525 * 864);
    }

    #[test]
    fn schedule_eras_and_boundaries() {
        assert_eq!(unlocked(-5), 0);
        assert_eq!(unlocked(0), 0);
        assert_eq!(unlocked(ERA_SECONDS / 2), ERA0_BUDGET / 2);
        assert_eq!(unlocked(ERA_SECONDS - 1), (ERA0_BUDGET as u128 * (ERA_SECONDS as u128 - 1) / ERA_SECONDS as u128) as u64);
        assert_eq!(unlocked(ERA_SECONDS), ERA0_BUDGET);
        assert_eq!(unlocked(2 * ERA_SECONDS), ERA0_BUDGET + ERA0_BUDGET / 2);
        assert_eq!(unlocked(3 * ERA_SECONDS), ERA0_BUDGET + ERA0_BUDGET / 2 + ERA0_BUDGET / 4);
        // Era 1 releases at exactly half the era-0 rate.
        let r0 = unlocked(ERA_SECONDS / 10);
        let r1 = unlocked(ERA_SECONDS + ERA_SECONDS / 10) - unlocked(ERA_SECONDS);
        assert_eq!(r1, r0 / 2);
        assert_eq!(era_at(ERA_SECONDS - 1), (0, ERA0_BUDGET));
        assert_eq!(era_at(ERA_SECONDS), (1, ERA0_BUDGET / 2));
        assert_eq!(era_at(2 * ERA_SECONDS), (2, ERA0_BUDGET / 4));
    }

    #[test]
    fn schedule_is_monotone_and_bounded_forever() {
        let dust = (ERA0_BUDGET.count_ones()) as u64;
        let mut previous = 0;
        for step in 0..2_000 {
            let t = step as i64 * (ERA_SECONDS / 7) + step as i64;
            let value = unlocked(t);
            assert!(value >= previous && value <= REWARD_RESERVE);
            previous = value;
        }
        assert_eq!(unlocked(i64::MAX), REWARD_RESERVE - dust);
        assert_eq!(unlocked(200 * ERA_SECONDS), REWARD_RESERVE - dust);
    }

    #[test]
    fn epoch_cap_is_bounded_by_new_unlocks_and_max_epoch() {
        let g = 1_000_000;
        // Fresh: one week after genesis, cap = everything unlocked so far.
        assert_eq!(epoch_reward_cap(g, g, g + MAX_EPOCH_SECONDS), unlocked(MAX_EPOCH_SECONDS));
        // A year without settlement: still at most MAX_EPOCH worth, never the era.
        let year = 31_557_600;
        let cap = epoch_reward_cap(g, g, g + year);
        assert_eq!(cap, unlocked(year) - unlocked(year - MAX_EPOCH_SECONDS));
        assert!(cap < ERA0_BUDGET / 200);
        // Right after a settlement, the cap restarts from zero.
        assert_eq!(epoch_reward_cap(g, g + year, g + year), 0);
        assert_eq!(epoch_reward_cap(g, g + year, g + year + 60), unlocked(year + 60) - unlocked(year));
        // Before genesis / clock skew: zero.
        assert_eq!(epoch_reward_cap(g, g, g - 5), 0);
    }

    #[test]
    fn burn_rounding() {
        assert_eq!(bounty_burn(10_000 * UNIT), 1_000 * UNIT);
        assert_eq!(bounty_burn(9), 0);
        assert_eq!(bounty_burn(10), 1);
        assert_eq!(bounty_burn(u64::MAX), (u64::MAX as u128 * 1_000 / 10_000) as u64);
    }

    #[test]
    fn merkle_matches_python_vector() {
        let claimant = Pubkey::new_from_array([7; 32]);
        let leaf = leaf_hash(0, &claimant, 1_000);
        let hex: String = leaf.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, include_str!("../tests/vector_leaf.txt").trim());
    }

    #[test]
    fn unpack_rejects_trailing_and_unknown() {
        let mut init = vec![0u8];
        init.extend_from_slice(&3600u64.to_le_bytes());
        assert!(LiteInstruction::unpack(&init).is_ok());
        init.push(0);
        assert!(LiteInstruction::unpack(&init).is_err());
        assert!(LiteInstruction::unpack(&[0]).is_err());
        assert!(LiteInstruction::unpack(&[9]).is_err());
        let mut long = vec![3u8];
        long.extend_from_slice(&1u64.to_le_bytes());
        long.push((MAX_PROOF_LEN + 1) as u8);
        assert!(LiteInstruction::unpack(&long).is_err());
    }
}
