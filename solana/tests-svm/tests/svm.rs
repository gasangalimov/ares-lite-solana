//! LiteSVM integration tests for the ARES Lite fixed-supply program (v2).
//!
//! Build first: `cd solana/program && cargo build-sbf`.
//! Instruction encodings, PDAs, Merkle hashing and the release schedule are
//! re-implemented here independently to cross-check the on-chain program.

use litesvm::LiteSVM;
use sha2::{Digest, Sha256};
use solana_account::Account;
use solana_address::Address as Pubkey;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_transaction::Transaction;

const TOKEN: Pubkey = solana_address::address!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const SYSTEM: Pubkey = solana_address::address!("11111111111111111111111111111111");
const LEAF_TAG: &[u8] = b"ARES-LITE/CLAIM-LEAF/v0";
const NODE_TAG: &[u8] = b"ARES-LITE/CLAIM-NODE/v0";

const UNIT: u64 = 1_000_000;
const TOTAL: u64 = 1_000_000_000 * UNIT;
const FOUNDER: u64 = 100_000_000 * UNIT;
const RESERVE: u64 = 900_000_000 * UNIT;
const ERA0: u64 = 450_000_000 * UNIT;
const ERA: i64 = 157_788_000;
const GENESIS_TS: i64 = 1_800_000_000;
const MAX_EPOCH: i64 = 7 * 86_400;
const CLAIM_DELAY: i64 = 3_600;

const ALREADY_INITIALIZED: u32 = 2;
const NOT_ADMIN: u32 = 3;
const BAD_MINT: u32 = 4;
const BAD_PDA: u32 = 5;
const CAP_EXCEEDED: u32 = 6;
const SEASON_EXISTS: u32 = 8;
const ROOT_ALREADY_PUBLISHED: u32 = 9;
const ROOT_NOT_PUBLISHED: u32 = 10;
const INVALID_PROOF: u32 = 11;
const ALREADY_CLAIMED: u32 = 12;
const SCHEDULE_EXCEEDED: u32 = 16;
const BOUNTY_CLOSED: u32 = 17;
const WRONG_PHASE: u32 = 18;
const LOG_HEAD_MISMATCH: u32 = 19;
const CLAIM_WINDOW_NOT_OPEN: u32 = 20;
const EPOCH_CAP_EXCEEDED: u32 = 21;

fn so_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../program/target/deploy/ares_lite_rewards.so")
}

// ------------------------------------------------------------------ helpers

fn unlocked(elapsed: i64) -> u64 {
    if elapsed <= 0 {
        return 0;
    }
    let era = (elapsed / ERA) as u64;
    let within = (elapsed % ERA) as u128;
    let mut total: u128 = (0..era.min(64)).map(|k| (ERA0 >> k) as u128).sum();
    if era < 64 {
        total += (ERA0 >> era) as u128 * within / ERA as u128;
    }
    total as u64
}

fn epoch_cap(last_settlement: i64, now: i64) -> u64 {
    let start = last_settlement.max(now - MAX_EPOCH).max(GENESIS_TS);
    if now <= start {
        return 0;
    }
    unlocked(now - GENESIS_TS) - unlocked(start - GENESIS_TS)
}

fn leaf(season: u64, claimant: &Pubkey, amount: u64) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update([0u8]);
    h.update(LEAF_TAG);
    h.update(season.to_le_bytes());
    h.update(claimant.as_ref());
    h.update(amount.to_le_bytes());
    h.finalize().into()
}

fn node(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    let mut h = Sha256::new();
    h.update([1u8]);
    h.update(NODE_TAG);
    h.update(lo);
    h.update(hi);
    h.finalize().into()
}

fn tree(leaves: &[[u8; 32]]) -> ([u8; 32], Vec<Vec<[u8; 32]>>) {
    let mut level = leaves.to_vec();
    let mut pos: Vec<usize> = (0..leaves.len()).collect();
    let mut proofs = vec![Vec::new(); leaves.len()];
    while level.len() > 1 {
        let mut next = Vec::new();
        for i in (0..level.len()).step_by(2) {
            next.push(if i + 1 < level.len() { node(&level[i], &level[i + 1]) } else { level[i] });
        }
        for (k, p) in pos.iter_mut().enumerate() {
            if *p ^ 1 < level.len() {
                proofs[k].push(level[*p ^ 1]);
            }
            *p /= 2;
        }
        level = next;
    }
    (level[0], proofs)
}

fn pda(seeds: &[&[u8]], program: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(seeds, program).0
}

struct Env {
    svm: LiteSVM,
    program: Pubkey,
    admin: Keypair,
    founder: Keypair,
    founder_ata: Pubkey,
    mint: Pubkey,
}

impl Env {
    fn config(&self) -> Pubkey {
        pda(&[b"config", self.mint.as_ref()], &self.program)
    }
    fn vault(&self) -> Pubkey {
        pda(&[b"vault", self.mint.as_ref()], &self.program)
    }
    fn vault_authority(&self) -> Pubkey {
        pda(&[b"vault_authority", self.mint.as_ref()], &self.program)
    }
    fn season(&self, id: u64) -> Pubkey {
        pda(&[b"season", self.config().as_ref(), &id.to_le_bytes()], &self.program)
    }
    fn bounty(&self, id: u64) -> Pubkey {
        pda(&[b"bounty", self.config().as_ref(), &id.to_le_bytes()], &self.program)
    }
    fn escrow(&self, id: u64) -> Pubkey {
        pda(&[b"escrow", self.bounty(id).as_ref()], &self.program)
    }
    fn send(&mut self, ixs: &[Instruction], payer: &Keypair, signers: &[&Keypair]) -> Result<(), String> {
        send(&mut self.svm, ixs, payer, signers)
    }
    fn now(&self) -> i64 {
        let clock: solana_clock::Clock = self.svm.get_sysvar();
        clock.unix_timestamp
    }
    fn set_time(&mut self, unix: i64) {
        let mut clock: solana_clock::Clock = self.svm.get_sysvar();
        clock.unix_timestamp = unix;
        self.svm.set_sysvar(&clock);
    }
}

fn send(svm: &mut LiteSVM, ixs: &[Instruction], payer: &Keypair, signers: &[&Keypair]) -> Result<(), String> {
    svm.expire_blockhash();
    let mut all: Vec<&Keypair> = vec![payer];
    all.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(ixs, Some(&payer.pubkey()), &all, svm.latest_blockhash());
    svm.send_transaction(tx).map(|_| ()).map_err(|e| format!("{:?}", e.err))
}

fn expect_err(result: Result<(), String>, code: u32) {
    let err = result.expect_err("transaction unexpectedly succeeded");
    assert_eq!(err, format!("InstructionError(0, Custom({code}))"), "unexpected error: {err}");
}

fn create_account(from: &Pubkey, to: &Pubkey, lamports: u64, space: u64, owner: &Pubkey) -> Instruction {
    let mut data = 0u32.to_le_bytes().to_vec();
    data.extend_from_slice(&lamports.to_le_bytes());
    data.extend_from_slice(&space.to_le_bytes());
    data.extend_from_slice(owner.as_ref());
    Instruction { program_id: SYSTEM, accounts: vec![AccountMeta::new(*from, true), AccountMeta::new(*to, true)], data }
}

fn create_mint(svm: &mut LiteSVM, payer: &Keypair, authority: &Pubkey, freeze: Option<&Pubkey>, decimals: u8) -> Pubkey {
    let mint = Keypair::new();
    let lamports = svm.minimum_balance_for_rent_exemption(82);
    let mut data = vec![20u8, decimals];
    data.extend_from_slice(authority.as_ref());
    match freeze {
        Some(f) => {
            data.push(1);
            data.extend_from_slice(f.as_ref());
        }
        None => {
            data.push(0);
            data.extend_from_slice(&[0; 32]);
        }
    }
    let init = Instruction { program_id: TOKEN, accounts: vec![AccountMeta::new(mint.pubkey(), false)], data };
    send(svm, &[create_account(&payer.pubkey(), &mint.pubkey(), lamports, 82, &TOKEN), init], payer, &[&mint]).unwrap();
    mint.pubkey()
}

fn token_account(svm: &mut LiteSVM, payer: &Keypair, mint: &Pubkey, owner: &Pubkey) -> Pubkey {
    let account = Keypair::new();
    let lamports = svm.minimum_balance_for_rent_exemption(165);
    let mut data = vec![18u8];
    data.extend_from_slice(owner.as_ref());
    let init = Instruction {
        program_id: TOKEN,
        accounts: vec![AccountMeta::new(account.pubkey(), false), AccountMeta::new_readonly(*mint, false)],
        data,
    };
    send(svm, &[create_account(&payer.pubkey(), &account.pubkey(), lamports, 165, &TOKEN), init], payer, &[&account]).unwrap();
    account.pubkey()
}

fn balance(svm: &LiteSVM, account: &Pubkey) -> u64 {
    u64::from_le_bytes(svm.get_account(account).unwrap().data[64..72].try_into().unwrap())
}

fn mint_state(svm: &LiteSVM, mint: &Pubkey) -> (Option<Pubkey>, u64, Option<Pubkey>) {
    let d = svm.get_account(mint).unwrap().data;
    let opt = |s: &[u8]| (u32::from_le_bytes(s[0..4].try_into().unwrap()) == 1).then(|| Pubkey::new_from_array(s[4..36].try_into().unwrap()));
    (opt(&d[0..36]), u64::from_le_bytes(d[36..44].try_into().unwrap()), opt(&d[46..82]))
}

fn token_ix(tag: u8, amount: u64, accounts: Vec<AccountMeta>) -> Instruction {
    let mut data = vec![tag];
    data.extend_from_slice(&amount.to_le_bytes());
    Instruction { program_id: TOKEN, accounts, data }
}

fn ix_initialize(program: &Pubkey, deployer: &Pubkey, mint: &Pubkey, founder_ata: &Pubkey) -> Instruction {
    Instruction {
        program_id: *program,
        accounts: vec![
            AccountMeta::new(*deployer, true),
            AccountMeta::new(pda(&[b"config", mint.as_ref()], program), false),
            AccountMeta::new(*mint, false),
            AccountMeta::new(pda(&[b"vault", mint.as_ref()], program), false),
            AccountMeta::new_readonly(pda(&[b"vault_authority", mint.as_ref()], program), false),
            AccountMeta::new(*founder_ata, false),
            AccountMeta::new_readonly(SYSTEM, false),
            AccountMeta::new_readonly(TOKEN, false),
        ],
        data: { let mut d = vec![0u8]; d.extend_from_slice(&(CLAIM_DELAY as u64).to_le_bytes()); d },
    }
}

fn ix_create_season(env: &Env, signer: &Pubkey, id: u64) -> Instruction {
    let mut data = vec![1u8];
    data.extend_from_slice(&id.to_le_bytes());
    data.extend_from_slice(&[0xAB; 32]);
    Instruction {
        program_id: env.program,
        accounts: vec![
            AccountMeta::new(*signer, true),
            AccountMeta::new_readonly(env.config(), false),
            AccountMeta::new(env.season(id), false),
            AccountMeta::new_readonly(SYSTEM, false),
        ],
        data,
    }
}

fn ix_close(env: &Env, signer: &Pubkey, id: u64, head: [u8; 32]) -> Instruction {
    let mut data = vec![7u8];
    data.extend_from_slice(&head);
    data.extend_from_slice(&1000u64.to_le_bytes());
    Instruction {
        program_id: env.program,
        accounts: vec![AccountMeta::new_readonly(*signer, true), AccountMeta::new(env.config(), false), AccountMeta::new(env.season(id), false)],
        data,
    }
}

fn ix_cancel(env: &Env, signer: &Pubkey, id: u64) -> Instruction {
    Instruction {
        program_id: env.program,
        accounts: vec![AccountMeta::new_readonly(*signer, true), AccountMeta::new(env.config(), false), AccountMeta::new(env.season(id), false)],
        data: vec![8],
    }
}

fn ix_publish(env: &Env, signer: &Pubkey, id: u64, root: [u8; 32], total: u64) -> Instruction {
    let mut data = vec![2u8];
    data.extend_from_slice(&root);
    data.extend_from_slice(&total.to_le_bytes());
    data.extend_from_slice(&[0xCD; 32]);
    data.extend_from_slice(&[0xEE; 32]);
    Instruction {
        program_id: env.program,
        accounts: vec![AccountMeta::new_readonly(*signer, true), AccountMeta::new(env.config(), false), AccountMeta::new(env.season(id), false)],
        data,
    }
}

fn ix_claim(env: &Env, claimant: &Pubkey, id: u64, amount: u64, proof: &[[u8; 32]], destination: &Pubkey) -> Instruction {
    let mut data = vec![3u8];
    data.extend_from_slice(&amount.to_le_bytes());
    data.push(proof.len() as u8);
    for p in proof {
        data.extend_from_slice(p);
    }
    let season = env.season(id);
    Instruction {
        program_id: env.program,
        accounts: vec![
            AccountMeta::new(*claimant, true),
            AccountMeta::new(env.config(), false),
            AccountMeta::new(season, false),
            AccountMeta::new(pda(&[b"claim", season.as_ref(), claimant.as_ref()], &env.program), false),
            AccountMeta::new(env.vault(), false),
            AccountMeta::new(*destination, false),
            AccountMeta::new_readonly(env.vault_authority(), false),
            AccountMeta::new_readonly(TOKEN, false),
            AccountMeta::new_readonly(SYSTEM, false),
        ],
        data,
    }
}

fn ix_fund_bounty(env: &Env, funder: &Pubkey, funder_ata: &Pubkey, id: u64, amount: u64) -> Instruction {
    let mut data = vec![5u8];
    data.extend_from_slice(&id.to_le_bytes());
    data.extend_from_slice(&amount.to_le_bytes());
    Instruction {
        program_id: env.program,
        accounts: vec![
            AccountMeta::new(*funder, true),
            AccountMeta::new(*funder_ata, false),
            AccountMeta::new(env.config(), false),
            AccountMeta::new(env.mint, false),
            AccountMeta::new(env.bounty(id), false),
            AccountMeta::new(env.escrow(id), false),
            AccountMeta::new_readonly(SYSTEM, false),
            AccountMeta::new_readonly(TOKEN, false),
        ],
        data,
    }
}

fn ix_award_bounty(env: &Env, signer: &Pubkey, id: u64, destination: &Pubkey) -> Instruction {
    Instruction {
        program_id: env.program,
        accounts: vec![
            AccountMeta::new_readonly(*signer, true),
            AccountMeta::new(env.config(), false),
            AccountMeta::new(env.bounty(id), false),
            AccountMeta::new(env.escrow(id), false),
            AccountMeta::new(*destination, false),
            AccountMeta::new_readonly(TOKEN, false),
        ],
        data: vec![6],
    }
}

fn base() -> (LiteSVM, Pubkey, Keypair) {
    let mut svm = LiteSVM::new();
    let program = Pubkey::new_unique();
    svm.add_program_from_file(program, so_path()).expect("build the program with `cargo build-sbf` first");
    let admin = Keypair::new();
    svm.airdrop(&admin.pubkey(), 100_000_000_000).unwrap();
    let mut clock: solana_clock::Clock = svm.get_sysvar();
    clock.unix_timestamp = GENESIS_TS;
    svm.set_sysvar(&clock);
    (svm, program, admin)
}

fn genesis() -> Env {
    let (mut svm, program, admin) = base();
    let founder = Keypair::new();
    svm.airdrop(&founder.pubkey(), 10_000_000_000).unwrap();
    let mint = create_mint(&mut svm, &admin, &admin.pubkey(), None, 6);
    let founder_ata = token_account(&mut svm, &admin, &mint, &founder.pubkey());
    send(&mut svm, &[ix_initialize(&program, &admin.pubkey(), &mint, &founder_ata)], &admin, &[]).unwrap();
    Env { svm, program, admin, founder, founder_ata, mint }
}

fn funded(env: &mut Env) -> Keypair {
    let k = Keypair::new();
    env.svm.airdrop(&k.pubkey(), 10_000_000_000).unwrap();
    k
}

fn season_with(env: &mut Env, id: u64, total: u64, awards: &[(Pubkey, u64)]) -> Result<Vec<Vec<[u8; 32]>>, String> {
    let proofs = publish_only(env, id, total, awards)?;
    let now = env.now();
    env.set_time(now + CLAIM_DELAY); // public verification window elapses
    Ok(proofs)
}

/// create + close (pin log head) + publish, without opening the claim window.
fn publish_only(env: &mut Env, id: u64, total: u64, awards: &[(Pubkey, u64)]) -> Result<Vec<Vec<[u8; 32]>>, String> {
    let admin = env.admin.insecure_clone();
    env.send(&[ix_create_season(env, &admin.pubkey(), id)], &admin, &[]).unwrap();
    env.send(&[ix_close(env, &admin.pubkey(), id, [0xCD; 32])], &admin, &[]).unwrap();
    let leaves: Vec<[u8; 32]> = awards.iter().map(|(k, a)| leaf(id, k, *a)).collect();
    let (root, proofs) = tree(&leaves);
    env.send(&[ix_publish(env, &admin.pubkey(), id, root, total)], &admin, &[])?;
    Ok(proofs)
}

// ================================================================ FIXED SUPPLY

#[test]
fn genesis_is_exactly_one_billion_split_10_90_and_mint_is_dead() {
    let env = genesis();
    let (authority, supply, freeze) = mint_state(&env.svm, &env.mint);
    assert_eq!(supply, TOTAL);
    assert_eq!(authority, None, "mint authority must be revoked");
    assert_eq!(freeze, None, "freeze authority must be None");
    assert_eq!(balance(&env.svm, &env.founder_ata), FOUNDER);
    assert_eq!(balance(&env.svm, &env.vault()), RESERVE);
    assert_eq!(FOUNDER * 10, TOTAL);
    // The vault is a token account owned by the vault-authority PDA.
    let vault = env.svm.get_account(&env.vault()).unwrap().data;
    assert_eq!(Pubkey::new_from_array(vault[32..64].try_into().unwrap()), env.vault_authority());
}

#[test]
fn mint_authority_death_test() {
    let mut env = genesis();
    let attacker = funded(&mut env);
    let target = env.founder_ata;
    let candidates: Vec<(Keypair, &str)> = vec![
        (env.founder.insecure_clone(), "founder"),
        (env.admin.insecure_clone(), "admin/deployer"),
        (attacker, "attacker"),
    ];
    for (signer, who) in candidates {
        let ix = token_ix(7, 1, vec![AccountMeta::new(env.mint, false), AccountMeta::new(target, false), AccountMeta::new_readonly(signer.pubkey(), true)]);
        assert!(env.send(&[ix], &signer, &[]).is_err(), "{who} could mint");
    }
    // PDAs cannot sign outside their program; the program has no mint path.
    let ix = token_ix(7, 1, vec![AccountMeta::new(env.mint, false), AccountMeta::new(target, false), AccountMeta::new_readonly(env.vault_authority(), false)]);
    let admin = env.admin.insecure_clone();
    assert!(env.send(&[ix], &admin, &[]).is_err(), "PDA mint");
    // Re-enabling minting via SetAuthority is impossible: no current authority.
    let mut data = vec![6u8, 0u8, 1u8];
    data.extend_from_slice(admin.pubkey().as_ref());
    let ix = Instruction { program_id: TOKEN, accounts: vec![AccountMeta::new(env.mint, false), AccountMeta::new_readonly(admin.pubkey(), true)], data };
    assert!(env.send(&[ix], &admin, &[]).is_err(), "authority re-set");
    assert_eq!(mint_state(&env.svm, &env.mint).1, TOTAL);
}

#[test]
fn genesis_rejects_bad_mints_wrong_signer_and_duplicates() {
    let (mut svm, program, admin) = base();
    let founder = Keypair::new();
    // Duplicate genesis.
    let mint = create_mint(&mut svm, &admin, &admin.pubkey(), None, 6);
    let ata = token_account(&mut svm, &admin, &mint, &founder.pubkey());
    send(&mut svm, &[ix_initialize(&program, &admin.pubkey(), &mint, &ata)], &admin, &[]).unwrap();
    expect_err(send(&mut svm, &[ix_initialize(&program, &admin.pubkey(), &mint, &ata)], &admin, &[]), ALREADY_INITIALIZED);
    // Freeze authority present / wrong decimals / non-authority signer.
    let frozen = create_mint(&mut svm, &admin, &admin.pubkey(), Some(&admin.pubkey()), 6);
    let f_ata = token_account(&mut svm, &admin, &frozen, &founder.pubkey());
    expect_err(send(&mut svm, &[ix_initialize(&program, &admin.pubkey(), &frozen, &f_ata)], &admin, &[]), BAD_MINT);
    let nine = create_mint(&mut svm, &admin, &admin.pubkey(), None, 9);
    let n_ata = token_account(&mut svm, &admin, &nine, &founder.pubkey());
    expect_err(send(&mut svm, &[ix_initialize(&program, &admin.pubkey(), &nine, &n_ata)], &admin, &[]), BAD_MINT);
    let intruder = Keypair::new();
    svm.airdrop(&intruder.pubkey(), 1_000_000_000).unwrap();
    let fresh = create_mint(&mut svm, &admin, &admin.pubkey(), None, 6);
    let fr_ata = token_account(&mut svm, &admin, &fresh, &founder.pubkey());
    expect_err(send(&mut svm, &[ix_initialize(&program, &intruder.pubkey(), &fresh, &fr_ata)], &intruder, &[]), BAD_MINT);
    // Pre-minted supply (a hidden premine) is rejected.
    let pre = create_mint(&mut svm, &admin, &admin.pubkey(), None, 6);
    let p_ata = token_account(&mut svm, &admin, &pre, &founder.pubkey());
    let ix = token_ix(7, 5, vec![AccountMeta::new(pre, false), AccountMeta::new(p_ata, false), AccountMeta::new_readonly(admin.pubkey(), true)]);
    send(&mut svm, &[ix], &admin, &[]).unwrap();
    expect_err(send(&mut svm, &[ix_initialize(&program, &admin.pubkey(), &pre, &p_ata)], &admin, &[]), BAD_MINT);
}

// ===================================================================== HALVING

#[test]
fn epoch_cap_separates_mining_epochs_from_the_halving_era() {
    let mut env = genesis();
    let admin = env.admin.insecure_clone();
    // At genesis nothing is unlocked.
    expect_err(season_with(&mut env, 0, 1, &[(admin.pubkey(), 1)]).map(|_| ()), EPOCH_CAP_EXCEEDED);
    // Half an era (2.5 years) without any settlement: the winner can still
    // only take one MAX_EPOCH (7 days) of release, never the 450M era budget.
    let t = GENESIS_TS + ERA / 2;
    env.set_time(t);
    let cap = epoch_cap(GENESIS_TS, t);
    assert_eq!(cap, unlocked(ERA / 2) - unlocked(ERA / 2 - MAX_EPOCH));
    // close_cap is fixed at close: waiting after close does not raise the epoch reward.
    env.send(&[ix_create_season(&env, &admin.pubkey(), 90)], &admin, &[]).unwrap();
    env.send(&[ix_close(&env, &admin.pubkey(), 90, [0xCD; 32])], &admin, &[]).unwrap();
    env.set_time(t + 3 * 86_400);
    let (root90, _) = tree(&[leaf(90, &admin.pubkey(), cap + 1)]);
    expect_err(env.send(&[ix_publish(&env, &admin.pubkey(), 90, root90, cap + 1)], &admin, &[]), EPOCH_CAP_EXCEEDED);
    env.set_time(t);
    assert!(cap < ERA0 / 250, "an epoch must be a tiny fraction of the era");
    expect_err(season_with(&mut env, 1, ERA0, &[(admin.pubkey(), ERA0)]).map(|_| ()), EPOCH_CAP_EXCEEDED);
    expect_err(season_with(&mut env, 2, cap + 1, &[(admin.pubkey(), cap + 1)]).map(|_| ()), EPOCH_CAP_EXCEEDED);
    season_with(&mut env, 3, cap, &[(admin.pubkey(), cap)]).unwrap();
    // Immediately after a settlement only the newly unlocked amount is available.
    let settled = t; // publish happened at t, then the claim window advanced the clock
    let now = env.now();
    let next = epoch_cap(settled, now);
    assert_eq!(next, unlocked(now - GENESIS_TS) - unlocked(settled - GENESIS_TS));
    expect_err(season_with(&mut env, 4, next + 1, &[(admin.pubkey(), next + 1)]).map(|_| ()), EPOCH_CAP_EXCEEDED);
}

#[test]
fn exact_five_year_halving_of_the_epoch_cap_on_chain() {
    let mut env = genesis();
    let admin = env.admin.insecure_clone();
    // Full week at the end of era 0 vs the first full week of era 1.
    env.set_time(GENESIS_TS + ERA);
    let era0_week = epoch_cap(GENESIS_TS, GENESIS_TS + ERA);
    expect_err(season_with(&mut env, 0, era0_week + 1, &[(admin.pubkey(), era0_week + 1)]).map(|_| ()), EPOCH_CAP_EXCEEDED);
    season_with(&mut env, 1, era0_week, &[(admin.pubkey(), era0_week)]).unwrap();
    env.set_time(GENESIS_TS + ERA + MAX_EPOCH);
    let era1_week = epoch_cap(GENESIS_TS + ERA, GENESIS_TS + ERA + MAX_EPOCH);
    assert!(era1_week.abs_diff(era0_week / 2) <= 1, "era 1 releases at half the era-0 rate");
    expect_err(season_with(&mut env, 2, era1_week + 1, &[(admin.pubkey(), era1_week + 1)]).map(|_| ()), EPOCH_CAP_EXCEEDED);
    season_with(&mut env, 3, era1_week, &[(admin.pubkey(), era1_week)]).unwrap();
    // Era 2 = 1/4 of the era-0 weekly rate.
    env.set_time(GENESIS_TS + 2 * ERA + MAX_EPOCH);
    let era2_week = epoch_cap(GENESIS_TS, GENESIS_TS + 2 * ERA + MAX_EPOCH);
    assert!(era2_week.abs_diff(era0_week / 4) <= 1);
    // Boundary: one second before vs at the halving, the per-second rate changes.
    assert_eq!(unlocked(ERA) - unlocked(ERA - 1), (ERA0 as u128 * ERA as u128 / ERA as u128 - ERA0 as u128 * (ERA as u128 - 1) / ERA as u128) as u64);
}

#[test]
fn long_horizon_never_exceeds_reserve_and_admin_cannot_change_schedule() {
    let mut env = genesis();
    let admin = env.admin.insecure_clone();
    env.set_time(GENESIS_TS + 300 * ERA);
    // Era >= 64: the release budget is exhausted, the cap is 0.
    assert_eq!(epoch_cap(GENESIS_TS, GENESIS_TS + 300 * ERA), 0);
    assert_eq!(unlocked(300 * ERA), RESERVE - u64::from(ERA0.count_ones()));
    expect_err(season_with(&mut env, 0, 1, &[(admin.pubkey(), 1)]).map(|_| ()), EPOCH_CAP_EXCEEDED);
    season_with(&mut env, 1, 0, &[(admin.pubkey(), 1)]).unwrap();
    for tag in [9u8, 10, 200] {
        let ix = Instruction { program_id: env.program, accounts: vec![AccountMeta::new(env.config(), false)], data: vec![tag] };
        assert!(env.send(&[ix], &admin, &[]).is_err());
    }
}

#[test]
fn deterministic_finalize_guards_log_head_window_and_cancel() {
    let mut env = genesis();
    env.set_time(GENESIS_TS + ERA / 2);
    let admin = env.admin.insecure_clone();
    let alice = funded(&mut env);
    let mallory = funded(&mut env);
    env.send(&[ix_create_season(&env, &admin.pubkey(), 0)], &admin, &[]).unwrap();
    // Publishing before the season is closed (log head pinned) is refused.
    expect_err(env.send(&[ix_publish(&env, &admin.pubkey(), 0, [1; 32], 1)], &admin, &[]), WRONG_PHASE);
    env.send(&[ix_close(&env, &admin.pubkey(), 0, [0x11; 32])], &admin, &[]).unwrap();
    expect_err(env.send(&[ix_close(&env, &admin.pubkey(), 0, [0x22; 32])], &admin, &[]), WRONG_PHASE);
    expect_err(env.send(&[ix_close(&env, &mallory.pubkey(), 0, [0x22; 32])], &mallory, &[]), NOT_ADMIN);
    // The root must be for the pinned head.
    let (root, proofs) = tree(&[leaf(0, &alice.pubkey(), 100)]);
    let mut wrong = ix_publish(&env, &admin.pubkey(), 0, root, 100);
    wrong.data[1 + 32 + 8..1 + 32 + 8 + 32].copy_from_slice(&[0x99; 32]);
    expect_err(env.send(&[wrong], &admin, &[]), LOG_HEAD_MISMATCH);
    let mut ok = ix_publish(&env, &admin.pubkey(), 0, root, 100);
    ok.data[1 + 32 + 8..1 + 32 + 8 + 32].copy_from_slice(&[0x11; 32]);
    env.send(&[ok], &admin, &[]).unwrap();
    // Claims wait for the public verification window.
    let mint = env.mint;
    let a = token_account(&mut env.svm, &alice, &mint, &alice.pubkey());
    let claim = ix_claim(&env, &alice.pubkey(), 0, 100, &proofs[0], &a);
    expect_err(env.send(&[claim.clone()], &alice, &[]), CLAIM_WINDOW_NOT_OPEN);
    // During the window a wrong root can be withdrawn (non-admin cannot).
    expect_err(env.send(&[ix_cancel(&env, &mallory.pubkey(), 0)], &mallory, &[]), NOT_ADMIN);
    env.send(&[ix_cancel(&env, &admin.pubkey(), 0)], &admin, &[]).unwrap();
    expect_err(env.send(&[claim.clone()], &alice, &[]), ROOT_NOT_PUBLISHED);
    // Republish (same pinned head), wait out the window, then claim; cancel is then impossible.
    let mut again = ix_publish(&env, &admin.pubkey(), 0, root, 100);
    again.data[1 + 32 + 8..1 + 32 + 8 + 32].copy_from_slice(&[0x11; 32]);
    env.send(&[again], &admin, &[]).unwrap();
    let now = env.now();
    env.set_time(now + CLAIM_DELAY);
    env.send(&[claim], &alice, &[]).unwrap();
    expect_err(env.send(&[ix_cancel(&env, &admin.pubkey(), 0)], &admin, &[]), WRONG_PHASE);
    assert_eq!(balance(&env.svm, &a), 100);
}

// ======================================================================= VAULT

#[test]
fn claims_transfer_from_vault_once_and_cannot_overdraft() {
    let mut env = genesis();
    env.set_time(GENESIS_TS + ERA);
    let alice = funded(&mut env);
    let bob = funded(&mut env);
    // A buggy tree sums to 900 while the committed total is 500.
    let proofs = season_with(&mut env, 0, 500, &[(alice.pubkey(), 450), (bob.pubkey(), 450)]).unwrap();
    let mint = env.mint;
    let a = token_account(&mut env.svm, &alice, &mint, &alice.pubkey());
    let b = token_account(&mut env.svm, &bob, &mint, &bob.pubkey());
    let ix = ix_claim(&env, &alice.pubkey(), 0, 450, &proofs[0], &a);
    env.send(&[ix.clone()], &alice, &[]).unwrap();
    expect_err(env.send(&[ix], &alice, &[]), ALREADY_CLAIMED);
    expect_err(env.send(&[ix_claim(&env, &bob.pubkey(), 0, 450, &proofs[1], &b)], &bob, &[]), CAP_EXCEEDED);
    assert_eq!(balance(&env.svm, &a), 450);
    assert_eq!(balance(&env.svm, &env.vault()), RESERVE - 450);
    assert_eq!(mint_state(&env.svm, &env.mint).1, TOTAL, "claims never change supply");
}

#[test]
fn founder_admin_and_attacker_cannot_drain_the_vault() {
    let mut env = genesis();
    env.set_time(GENESIS_TS + ERA);
    let attacker = funded(&mut env);
    let mint = env.mint;
    let sink = token_account(&mut env.svm, &attacker, &mint, &attacker.pubkey());
    for signer in [env.founder.insecure_clone(), env.admin.insecure_clone(), attacker.insecure_clone()] {
        let ix = token_ix(3, 1, vec![AccountMeta::new(env.vault(), false), AccountMeta::new(sink, false), AccountMeta::new_readonly(signer.pubkey(), true)]);
        assert!(env.send(&[ix], &signer, &[]).is_err());
    }
    // Claim with a forged proof / fake vault authority.
    let alice = funded(&mut env);
    let proofs = season_with(&mut env, 0, 100, &[(alice.pubkey(), 100)]).unwrap();
    expect_err(env.send(&[ix_claim(&env, &attacker.pubkey(), 0, 100, &proofs[0], &sink)], &attacker, &[]), INVALID_PROOF);
    let mut ix = ix_claim(&env, &attacker.pubkey(), 0, 100, &proofs[0], &sink);
    ix.accounts[6] = AccountMeta::new_readonly(attacker.pubkey(), false);
    expect_err(env.send(&[ix], &attacker, &[]), BAD_PDA);
    // Admin cannot publish beyond the schedule (no early release of future reserve).
    let admin = env.admin.insecure_clone();
    expect_err(season_with(&mut env, 1, ERA0, &[(admin.pubkey(), ERA0)]).map(|_| ()), EPOCH_CAP_EXCEEDED);
    assert_eq!(balance(&env.svm, &env.vault()), RESERVE);
}

#[test]
fn seasons_admin_only_roots_immutable_wrong_season_and_wallet_rejected() {
    let mut env = genesis();
    env.set_time(GENESIS_TS + ERA);
    let admin = env.admin.insecure_clone();
    let alice = funded(&mut env);
    let mallory = funded(&mut env);
    expect_err(env.send(&[ix_create_season(&env, &mallory.pubkey(), 0)], &mallory, &[]), NOT_ADMIN);
    let proofs = season_with(&mut env, 0, 100, &[(alice.pubkey(), 100)]).unwrap();
    expect_err(env.send(&[ix_create_season(&env, &admin.pubkey(), 0)], &admin, &[]), SEASON_EXISTS);
    let mut again = ix_publish(&env, &admin.pubkey(), 0, [9; 32], 1);
    again.data[1 + 32 + 8..1 + 32 + 8 + 32].copy_from_slice(&[0xCD; 32]);
    expect_err(env.send(&[again], &admin, &[]), ROOT_ALREADY_PUBLISHED);
    env.send(&[ix_create_season(&env, &admin.pubkey(), 1)], &admin, &[]).unwrap();
    let mint = env.mint;
    let a = token_account(&mut env.svm, &alice, &mint, &alice.pubkey());
    let m = token_account(&mut env.svm, &mallory, &mint, &mallory.pubkey());
    expect_err(env.send(&[ix_claim(&env, &alice.pubkey(), 1, 100, &proofs[0], &a)], &alice, &[]), ROOT_NOT_PUBLISHED);
    expect_err(env.send(&[ix_claim(&env, &mallory.pubkey(), 0, 100, &proofs[0], &m)], &mallory, &[]), INVALID_PROOF);
    expect_err(env.send(&[ix_publish(&env, &mallory.pubkey(), 1, [1; 32], 1)], &mallory, &[]), NOT_ADMIN);
    // Season 1 is still OPEN (not closed): publishing is refused even for the admin.
    expect_err(env.send(&[ix_publish(&env, &admin.pubkey(), 1, [1; 32], 1)], &admin, &[]), WRONG_PHASE);
}

// ======================================================================== BURN

#[test]
fn bounty_burns_exactly_ten_percent_and_pays_the_rest_once() {
    let mut env = genesis();
    let founder = env.founder.insecure_clone();
    let admin = env.admin.insecure_clone();
    let solver = funded(&mut env);
    let mint = env.mint;
    let solver_ata = token_account(&mut env.svm, &solver, &mint, &solver.pubkey());
    let amount = 10_000 * UNIT;
    let before_supply = mint_state(&env.svm, &env.mint).1;
    let before_founder = balance(&env.svm, &env.founder_ata);
    let ata = env.founder_ata;
    env.send(&[ix_fund_bounty(&env, &founder.pubkey(), &ata, 7, amount)], &founder, &[]).unwrap();
    assert_eq!(mint_state(&env.svm, &env.mint).1, before_supply - 1_000 * UNIT, "supply must drop by exactly the burn");
    assert_eq!(balance(&env.svm, &env.founder_ata), before_founder - amount);
    assert_eq!(balance(&env.svm, &env.escrow(7)), 9_000 * UNIT);
    // Same bounty id cannot be funded twice (no double burn).
    expect_err(env.send(&[ix_fund_bounty(&env, &founder.pubkey(), &ata, 7, amount)], &founder, &[]), ALREADY_INITIALIZED);
    // Only the admin awards, exactly once.
    expect_err(env.send(&[ix_award_bounty(&env, &solver.pubkey(), 7, &solver_ata)], &solver, &[]), NOT_ADMIN);
    env.send(&[ix_award_bounty(&env, &admin.pubkey(), 7, &solver_ata)], &admin, &[]).unwrap();
    assert_eq!(balance(&env.svm, &solver_ata), 9_000 * UNIT);
    expect_err(env.send(&[ix_award_bounty(&env, &admin.pubkey(), 7, &solver_ata)], &admin, &[]), BOUNTY_CLOSED);
    assert_eq!(mint_state(&env.svm, &env.mint).1, TOTAL - 1_000 * UNIT);
}

#[test]
fn burn_rounding_and_no_burning_of_other_peoples_tokens() {
    let mut env = genesis();
    let founder = env.founder.insecure_clone();
    let admin = env.admin.insecure_clone();
    let ata = env.founder_ata;
    // 9 base units: burn floor(0.9) = 0, all 9 escrowed.
    env.send(&[ix_fund_bounty(&env, &founder.pubkey(), &ata, 1, 9)], &founder, &[]).unwrap();
    assert_eq!(mint_state(&env.svm, &env.mint).1, TOTAL);
    assert_eq!(balance(&env.svm, &env.escrow(1)), 9);
    // The admin cannot fund (and thereby burn) from the founder's account.
    let r = env.send(&[ix_fund_bounty(&env, &admin.pubkey(), &ata, 2, 1_000)], &admin, &[]);
    assert!(r.is_err());
    // Nor burn directly from someone else's account or the vault.
    for (account, signer) in [(ata, admin.insecure_clone()), (env.vault(), admin.insecure_clone()), (env.vault(), founder.insecure_clone())] {
        let ix = token_ix(8, 1, vec![AccountMeta::new(account, false), AccountMeta::new(env.mint, false), AccountMeta::new_readonly(signer.pubkey(), true)]);
        assert!(env.send(&[ix], &signer, &[]).is_err());
    }
    assert_eq!(mint_state(&env.svm, &env.mint).1, TOTAL);
}

// ======================================================================== POOLS

#[test]
fn pool_addresses_and_multiple_wallets_are_ordinary_claimants() {
    let mut env = genesis();
    env.set_time(GENESIS_TS + ERA / 4);
    let pool = funded(&mut env); // any address: a pool's payout wallet or multisig
    let same_owner_wallet = funded(&mut env);
    let proofs = season_with(&mut env, 0, 3_000, &[(pool.pubkey(), 2_000), (same_owner_wallet.pubkey(), 1_000)]).unwrap();
    let mint = env.mint;
    let p = token_account(&mut env.svm, &pool, &mint, &pool.pubkey());
    let w = token_account(&mut env.svm, &same_owner_wallet, &mint, &same_owner_wallet.pubkey());
    env.send(&[ix_claim(&env, &pool.pubkey(), 0, 2_000, &proofs[0], &p)], &pool, &[]).unwrap();
    env.send(&[ix_claim(&env, &same_owner_wallet.pubkey(), 0, 1_000, &proofs[1], &w)], &same_owner_wallet, &[]).unwrap();
    assert_eq!(balance(&env.svm, &p), 2_000);
    assert_eq!(balance(&env.svm, &w), 1_000);
}

#[test]
fn prefunded_receipt_and_admin_handoff() {
    let mut env = genesis();
    env.set_time(GENESIS_TS + ERA);
    let alice = funded(&mut env);
    let proofs = season_with(&mut env, 0, 100, &[(alice.pubkey(), 100)]).unwrap();
    let season = env.season(0);
    let receipt = pda(&[b"claim", season.as_ref(), alice.pubkey().as_ref()], &env.program);
    env.svm.set_account(receipt, Account { lamports: 1, data: vec![], owner: SYSTEM, executable: false, rent_epoch: 0 }).unwrap();
    let mint = env.mint;
    let a = token_account(&mut env.svm, &alice, &mint, &alice.pubkey());
    env.send(&[ix_claim(&env, &alice.pubkey(), 0, 100, &proofs[0], &a)], &alice, &[]).unwrap();
    // Admin handoff to a multisig key.
    let admin = env.admin.insecure_clone();
    let multisig = funded(&mut env);
    let mut data = vec![4u8];
    data.extend_from_slice(multisig.pubkey().as_ref());
    let ix = Instruction { program_id: env.program, accounts: vec![AccountMeta::new_readonly(admin.pubkey(), true), AccountMeta::new(env.config(), false)], data };
    env.send(&[ix], &admin, &[]).unwrap();
    expect_err(env.send(&[ix_create_season(&env, &admin.pubkey(), 1)], &admin, &[]), NOT_ADMIN);
    env.send(&[ix_create_season(&env, &multisig.pubkey(), 1)], &multisig, &[]).unwrap();
}

#[test]
fn python_and_rust_claim_leaves_agree() {
    let expected = include_str!("../../program/tests/vector_leaf.txt").trim();
    let got: String = leaf(0, &Pubkey::new_from_array([7; 32]), 1_000).iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(got, expected);
}

#[test]
fn vault_cannot_be_bypassed_by_substituted_accounts_or_redirected_claims() {
    let mut env = genesis();
    env.set_time(GENESIS_TS + ERA);
    let alice = funded(&mut env);
    let attacker = funded(&mut env);
    let proofs = season_with(&mut env, 0, 100, &[(alice.pubkey(), 100)]).unwrap();
    let mint = env.mint;
    let a = token_account(&mut env.svm, &alice, &mint, &alice.pubkey());
    let evil = token_account(&mut env.svm, &attacker, &mint, &attacker.pubkey());
    // Substituted vault (attacker's own token account) is refused.
    let mut ix = ix_claim(&env, &alice.pubkey(), 0, 100, &proofs[0], &a);
    ix.accounts[4] = AccountMeta::new(evil, false);
    expect_err(env.send(&[ix], &alice, &[]), BAD_PDA);
    // Fake config account (not owned by the program) is refused.
    let fake = Pubkey::new_unique();
    let real = env.svm.get_account(&env.config()).unwrap();
    env.svm.set_account(fake, Account { owner: SYSTEM, ..real }).unwrap();
    let mut ix = ix_claim(&env, &alice.pubkey(), 0, 100, &proofs[0], &a);
    ix.accounts[1] = AccountMeta::new(fake, false);
    assert!(env.send(&[ix], &alice, &[]).is_err());
    // Nobody can claim Alice's reward to their own account without Alice's signature.
    let mut ix = ix_claim(&env, &alice.pubkey(), 0, 100, &proofs[0], &evil);
    ix.accounts[0].is_signer = false;
    assert!(env.send(&[ix], &attacker, &[]).is_err());
    // Alice claims normally; the vault decreases by exactly the claim.
    env.send(&[ix_claim(&env, &alice.pubkey(), 0, 100, &proofs[0], &a)], &alice, &[]).unwrap();
    assert_eq!(balance(&env.svm, &env.vault()), RESERVE - 100);
    assert_eq!(balance(&env.svm, &evil), 0);
}
