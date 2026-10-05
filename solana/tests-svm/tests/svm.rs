//! LiteSVM integration tests for the ARES Lite program v4 (FROZEN ECONOMICS:
//! ARES_LITE_FINAL_ECONOMICS_SPEC_v1 + R1 + R2).
//!
//! Build first:
//!   cd lite/solana/program && cargo build-sbf
//!   cargo build-sbf --features devnet-time-scale --sbf-out-dir target/deploy-devnet
//!
//! Instruction encodings, PDAs, account layouts, Merkle hashing and the
//! economics (cap, vesting, bounty split) are re-implemented here
//! independently. An integer reference model (`Model`) mirrors the spec and
//! every on-chain step is followed by `check()`: model == chain and
//! invariants 1–13 hold.

use litesvm::LiteSVM;
use sha2::{Digest, Sha256};
use solana_account::Account;
use solana_address::Address as Pubkey;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_transaction::Transaction;
use std::collections::BTreeMap;

const TOKEN: Pubkey = solana_address::address!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const SYSTEM: Pubkey = solana_address::address!("11111111111111111111111111111111");
const LEAF_TAG: &[u8] = b"ARES-LITE/CLAIM-LEAF/v0";
const NODE_TAG: &[u8] = b"ARES-LITE/CLAIM-NODE/v0";

const UNIT: u64 = 1_000_000;
const MAX: u64 = 100_000_000 * UNIT;
const FOUNDER: u64 = 10_000_000 * UNIT;
const MINING: u64 = 90_000_000 * UNIT;
const DAY: i64 = 86_400;
const YEAR: i64 = 365 * DAY;
const REVEAL_END: i64 = 79_200;
const CLOSE_GRACE: i64 = 21_600;
const PUBLISH_WINDOW: i64 = 172_800;
const CLAIM_DELAY: i64 = 259_200;
const T0: i64 = 1_800_000_000;

const ALREADY_INITIALIZED: u32 = 2;
const NOT_ADMIN: u32 = 3;
const BAD_MINT: u32 = 4;
const BAD_PDA: u32 = 5;
const CAP_EXCEEDED: u32 = 6;
const EPOCH_EXISTS: u32 = 8;
const ROOT_ALREADY_PUBLISHED: u32 = 9;
const NOT_FINAL: u32 = 10;
const INVALID_PROOF: u32 = 11;
const ALREADY_CLAIMED: u32 = 12;
const BAD_ACCOUNT: u32 = 13;
const BOUNTY_TOO_SMALL: u32 = 16;
const BOUNTY_CLOSED: u32 = 17;
const WRONG_PHASE: u32 = 18;
const LOG_HEAD_MISMATCH: u32 = 19;
const CLAIM_WINDOW_NOT_OPEN: u32 = 20;
const EPOCH_CAP_EXCEEDED: u32 = 21;
const OUTSIDE_CLOSE_WINDOW: u32 = 22;
const EPOCH_ORDER: u32 = 23;
const DEADLINE_PASSED: u32 = 24;
const DEADLINE_NOT_REACHED: u32 = 25;
const CORRECTION_USED: u32 = 26;
const NOT_BENEFICIARY: u32 = 27;
const NOTHING_VESTED: u32 = 28;
const RESERVE_UNDERFLOW: u32 = 29;
const BAD_EPOCH_SECONDS: u32 = 30;

const HEAD: [u8; 32] = [0xCD; 32];

fn so_path(devnet: bool) -> std::path::PathBuf {
    let dir = if devnet { "deploy-devnet" } else { "deploy" };
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../program/target/{dir}/ares_lite_rewards.so"))
}

// ------------------------------------------------------- independent economics

fn cap_of(available: u64) -> u64 {
    (available as u128 * 379_735 / 1_000_000_000) as u64
}

fn vested_at(genesis: i64, epoch_seconds: i64, now: i64) -> u64 {
    let y = 365 * epoch_seconds;
    if now < genesis + y {
        0
    } else if now >= genesis + 5 * y {
        FOUNDER
    } else {
        (FOUNDER as u128 * (now - genesis - y) as u128 / (4 * y) as u128) as u64
    }
}

// --------------------------------------------------------------------- merkle

fn leaf(epoch: u64, claimant: &Pubkey, amount: u64) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update([0u8]);
    h.update(LEAF_TAG);
    h.update(epoch.to_le_bytes());
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

// ---------------------------------------------------------------- the model

const OPEN: u8 = 0;
const CLOSED: u8 = 1;
const PUBLISHED: u8 = 2;
const WITHDRAWN: u8 = 3;
const CORRECTED: u8 = 4;
const FINAL: u8 = 5;
const EXPIRED: u8 = 6;

#[derive(Clone, Debug, Default, PartialEq)]
struct MEpoch {
    status: u8,
    cap: u64,
    total: u64,
    claimed: u64,
    deadline: i64,
    open_ts: i64,
    corrections: u8,
    orig_total: u64,
}

#[derive(Clone, Debug, Default)]
struct Model {
    v: u64,
    p: u64,
    o: u64,
    recycled: u64,
    claimed_total: u64,
    awarded: u64,
    founder_claimed: u64,
    escrow: u64,
    last_closed: Option<u64>,
    epochs: BTreeMap<u64, MEpoch>,
}

impl Model {
    fn avail(&self) -> u64 {
        self.v - self.p - self.o
    }
}

// ----------------------------------------------------------------- the chain

struct Env {
    svm: LiteSVM,
    program: Pubkey,
    admin: Keypair,
    founder: Keypair,
    founder_ata: Pubkey,
    mint: Pubkey,
    payer: Keypair,
    epoch_seconds: i64,
    m: Model,
    /// every non-program token account (W) for the conservation check
    wallets: Vec<Pubkey>,
    escrows: Vec<Pubkey>,
    /// (epoch, winner, amount) of the single-leaf result currently published
    winners: BTreeMap<u64, Keypair>,
}

struct Ch {
    p: u64,
    o: u64,
    last_closed: u64,
    recycled: u64,
    claimed_total: u64,
    founder_claimed: u64,
    escrowed: u64,
    awarded: u64,
    genesis: i64,
    epoch_seconds: i64,
    admin: Pubkey,
    beneficiary: Pubkey,
}

fn rd(d: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(d[at..at + 8].try_into().unwrap())
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
    fn founder_vault(&self) -> Pubkey {
        pda(&[b"founder_vault", self.mint.as_ref()], &self.program)
    }
    fn founder_authority(&self) -> Pubkey {
        pda(&[b"founder_authority", self.mint.as_ref()], &self.program)
    }
    fn epoch(&self, e: u64) -> Pubkey {
        pda(&[b"epoch", self.config().as_ref(), &e.to_le_bytes()], &self.program)
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
        clock.slot += 1;
        self.svm.set_sysvar(&clock);
    }
    fn scaled(&self, prod: i64) -> i64 {
        prod * self.epoch_seconds / DAY
    }
    fn start(&self, e: u64) -> i64 {
        T0 + e as i64 * self.epoch_seconds
    }
    fn close_time(&self, e: u64) -> i64 {
        self.start(e) + self.scaled(REVEAL_END)
    }
    fn chain(&self) -> Ch {
        let d = self.svm.get_account(&self.config()).unwrap().data;
        assert_eq!(d.len(), 284);
        assert_eq!(d[0], 41);
        Ch {
            admin: Pubkey::new_from_array(d[2..34].try_into().unwrap()),
            beneficiary: Pubkey::new_from_array(d[130..162].try_into().unwrap()),
            genesis: rd(&d, 162) as i64,
            epoch_seconds: rd(&d, 170) as i64,
            p: rd(&d, 178),
            o: rd(&d, 186),
            last_closed: rd(&d, 194),
            recycled: rd(&d, 202),
            claimed_total: rd(&d, 210),
            founder_claimed: rd(&d, 218),
            escrowed: rd(&d, 226),
            awarded: rd(&d, 242),
        }
    }
    fn chain_epoch(&self, e: u64) -> MEpoch {
        let d = self.svm.get_account(&self.epoch(e)).unwrap().data;
        assert_eq!(d.len(), 357);
        assert_eq!(rd(&d, 2), e);
        MEpoch {
            status: d[1],
            cap: rd(&d, 82),
            total: rd(&d, 122),
            claimed: rd(&d, 162),
            deadline: rd(&d, 170) as i64,
            open_ts: rd(&d, 178) as i64,
            corrections: d[258],
            orig_total: rd(&d, 218),
        }
    }
    fn avail(&self) -> u64 {
        let c = self.chain();
        balance(&self.svm, &self.vault()) - c.p - c.o
    }

    /// model == chain and invariants 1–13.
    fn check(&self) {
        let m = &self.m;
        let c = self.chain();
        let (mint_auth, supply, freeze) = mint_state(&self.svm, &self.mint);
        let v = balance(&self.svm, &self.vault());
        let fv = balance(&self.svm, &self.founder_vault());
        let e: u64 = self.escrows.iter().map(|k| balance(&self.svm, k)).sum();
        let w: u64 = self.wallets.iter().map(|k| balance(&self.svm, k)).sum();
        // (1) supply and authorities
        assert_eq!((mint_auth, supply, freeze), (None, MAX, None), "inv1");
        // (2) conservation
        assert_eq!(v + fv + e + w, MAX, "inv2 conservation");
        // model == chain
        assert_eq!((v, c.p, c.o), (m.v, m.p, m.o), "V,P,O");
        assert_eq!((c.recycled, c.claimed_total, c.awarded, c.founder_claimed, c.escrowed), (m.recycled, m.claimed_total, m.awarded, m.founder_claimed, m.escrow));
        assert_eq!(e, m.escrow);
        assert_eq!(c.last_closed, m.last_closed.unwrap_or(u64::MAX), "inv8 last_closed");
        // (3) V >= P + O
        assert!(v >= c.p + c.o, "inv3");
        // (4') P = Σ cap over HOLD; O = Σ (total - claimed) over FINAL
        let mut p = 0;
        let mut o = 0;
        let mut awarded = 0;
        for (k, me) in &m.epochs {
            let ce = self.chain_epoch(*k);
            assert_eq!(&ce, me, "epoch {k} model != chain");
            if matches!(me.status, CLOSED | PUBLISHED | WITHDRAWN | CORRECTED) {
                p += me.cap;
            }
            if me.status == FINAL {
                o += me.total - me.claimed;
                awarded += me.total;
            }
            // (6) and (13)
            assert!(me.total <= me.cap && me.claimed <= me.total, "inv6");
            assert!(me.corrections <= 1);
            if me.claimed > 0 {
                assert_eq!(me.status, FINAL, "inv13");
            }
            if me.status == EXPIRED {
                assert_eq!((me.total, me.claimed), (0, 0), "inv13 expired");
            }
        }
        assert_eq!((c.p, c.o), (p, o), "inv4'");
        assert_eq!(c.awarded, awarded);
        // (5') exact vault ledger (no direct donations in these scenarios)
        assert_eq!(v + c.claimed_total, MINING + c.recycled, "inv5'");
        // (9) founder vesting
        let vested = vested_at(c.genesis, c.epoch_seconds, self.now());
        assert!(c.founder_claimed <= vested && vested <= FOUNDER, "inv9");
        assert_eq!(fv, FOUNDER - c.founder_claimed, "founder vault");
        // (12) only awards deplete
        assert_eq!(v - c.p - c.o, MINING + c.recycled - awarded - c.p, "inv12");
    }

    // ---------------------------------------------------------- operations

    fn admin(&self) -> Keypair {
        self.admin.insecure_clone()
    }

    fn try_create(&mut self, e: u64) -> Result<(), String> {
        let a = self.admin();
        let r = self.send(&[ix_create_epoch(self, &a.pubkey(), e)], &a, &[]);
        if r.is_ok() {
            self.m.epochs.insert(e, MEpoch::default());
        }
        r
    }

    fn try_close(&mut self, e: u64) -> Result<u64, String> {
        if !self.m.epochs.contains_key(&e) {
            self.try_create(e)?;
        }
        let a = self.admin();
        let cap = cap_of(self.m.avail());
        self.send(&[ix_close(self, &a.pubkey(), e, HEAD)], &a, &[])?;
        let now = self.now();
        let deadline = now + self.scaled(PUBLISH_WINDOW);
        let me = self.m.epochs.get_mut(&e).unwrap();
        *me = MEpoch { status: CLOSED, cap, deadline, ..Default::default() };
        self.m.p += cap;
        self.m.last_closed = Some(e);
        self.check();
        Ok(cap)
    }

    fn close(&mut self, e: u64) -> u64 {
        let t = self.close_time(e);
        if self.now() < t {
            self.set_time(t);
        }
        self.try_close(e).unwrap()
    }

    fn try_publish(&mut self, e: u64, total: u64, correction: bool) -> Result<(), String> {
        let a = self.admin();
        let winner = Keypair::new();
        let root = leaf(e, &winner.pubkey(), total);
        let ix = if correction { ix_republish(self, &a.pubkey(), e, root, total) } else { ix_publish(self, &a.pubkey(), e, root, total) };
        self.send(&[ix], &a, &[])?;
        let open = self.now() + self.scaled(CLAIM_DELAY);
        let me = self.m.epochs.get_mut(&e).unwrap();
        me.total = total;
        me.open_ts = open;
        me.deadline = 0;
        if correction {
            me.status = CORRECTED;
            me.corrections = 1;
        } else {
            me.status = PUBLISHED;
        }
        self.winners.insert(e, winner);
        self.check();
        Ok(())
    }

    fn try_withdraw(&mut self, e: u64) -> Result<(), String> {
        let a = self.admin();
        self.send(&[ix_withdraw(self, &a.pubkey(), e)], &a, &[])?;
        let me = self.m.epochs.get_mut(&e).unwrap();
        me.orig_total = me.total;
        me.total = 0;
        me.deadline = me.open_ts;
        me.open_ts = 0;
        me.corrections = 1;
        me.status = WITHDRAWN;
        self.winners.remove(&e);
        self.check();
        Ok(())
    }

    fn try_expire(&mut self, e: u64) -> Result<(), String> {
        let p = self.payer.insecure_clone();
        self.send(&[ix_expire(self, e)], &p, &[])?;
        let me = self.m.epochs.get_mut(&e).unwrap();
        me.status = EXPIRED;
        me.total = 0;
        let cap = me.cap;
        self.m.p -= cap;
        self.check();
        Ok(())
    }

    fn try_finalize(&mut self, e: u64) -> Result<(), String> {
        let p = self.payer.insecure_clone();
        self.send(&[ix_finalize(self, e)], &p, &[])?;
        let me = self.m.epochs.get_mut(&e).unwrap();
        me.status = FINAL;
        let (cap, total) = (me.cap, me.total);
        self.m.p -= cap;
        self.m.o += total;
        self.m.awarded += total;
        self.check();
        Ok(())
    }

    /// The single-leaf winner of epoch e claims its full total.
    fn try_claim_winner(&mut self, e: u64) -> Result<(), String> {
        let winner = self.winners.get(&e).expect("no winner").insecure_clone();
        let total = self.m.epochs[&e].total;
        if self.svm.get_balance(&winner.pubkey()).unwrap_or(0) == 0 {
            self.svm.airdrop(&winner.pubkey(), 1_000_000_000).unwrap();
        }
        let dest = self.wallet_for(&winner);
        let ix = ix_claim(self, &winner.pubkey(), e, total, &[], &dest);
        self.send(&[ix], &winner, &[])?;
        self.m.epochs.get_mut(&e).unwrap().claimed += total;
        self.m.o -= total;
        self.m.v -= total;
        self.m.claimed_total += total;
        self.check();
        Ok(())
    }

    fn wallet_for(&mut self, owner: &Keypair) -> Pubkey {
        let p = self.payer.insecure_clone();
        let mint = self.mint;
        let a = token_account(&mut self.svm, &p, &mint, &owner.pubkey());
        self.wallets.push(a);
        a
    }

    fn try_release(&mut self) -> Result<u64, String> {
        let f = self.founder.insecure_clone();
        let ata = self.founder_ata;
        let before = balance(&self.svm, &ata);
        self.send(&[ix_release(self, &f.pubkey(), &ata)], &f, &[])?;
        let got = balance(&self.svm, &ata) - before;
        let vested = vested_at(T0, self.epoch_seconds, self.now());
        assert_eq!(got, vested - self.m.founder_claimed, "release amount");
        self.m.founder_claimed = vested;
        self.check();
        Ok(got)
    }

    fn try_fund_bounty(&mut self, id: u64, amount: u64) -> Result<(u64, u64), String> {
        let f = self.founder.insecure_clone();
        let ata = self.founder_ata;
        self.send(&[ix_fund_bounty(self, &f.pubkey(), &ata, id, amount)], &f, &[])?;
        let fee = amount / 20;
        self.m.v += fee;
        self.m.recycled += fee;
        self.m.escrow += amount - fee;
        let esc = self.escrow(id);
        self.escrows.push(esc);
        self.check();
        Ok((fee, amount - fee))
    }

    /// Settle every epoch whose time has come (finalize+claim, or expire).
    fn settle_due(&mut self) {
        let now = self.now();
        let due: Vec<(u64, MEpoch)> = self.m.epochs.iter().map(|(k, v)| (*k, v.clone())).collect();
        for (e, me) in due {
            if matches!(me.status, PUBLISHED | CORRECTED) && now >= me.open_ts {
                self.try_finalize(e).unwrap();
                if me.total > 0 {
                    self.try_claim_winner(e).unwrap();
                }
            } else if matches!(me.status, CLOSED | WITHDRAWN) && now >= me.deadline {
                self.try_expire(e).unwrap();
            }
        }
    }

    /// Move time forward until nothing is held, settling everything.
    fn drain(&mut self) {
        loop {
            let next = self
                .m
                .epochs
                .values()
                .filter_map(|me| match me.status {
                    PUBLISHED | CORRECTED => Some(me.open_ts),
                    CLOSED | WITHDRAWN => Some(me.deadline),
                    _ => None,
                })
                .min();
            match next {
                None => break,
                Some(t) => {
                    if self.now() < t {
                        self.set_time(t);
                    }
                    self.settle_due();
                }
            }
        }
        assert_eq!(self.m.p, 0);
    }

    /// Realistic 24 h cadence (the reference model's `parallel()`): at
    /// close_time(e) settle what is due, close e, publish 1 h later
    /// (total = total_fn(cap)) unless e is in `fail`.
    fn cadence(&mut self, epochs: std::ops::Range<u64>, fail: &[u64], total_fn: &dyn Fn(u64) -> u64) -> Vec<u64> {
        let mut caps = Vec::new();
        for e in epochs {
            let t = self.close_time(e);
            self.set_time(t);
            self.settle_due();
            let cap = self.try_close(e).unwrap();
            caps.push(cap);
            if !fail.contains(&e) {
                self.set_time(t + self.scaled(3_600));
                self.try_publish(e, total_fn(cap), false).unwrap();
            }
        }
        caps
    }
}

fn send(svm: &mut LiteSVM, ixs: &[Instruction], payer: &Keypair, signers: &[&Keypair]) -> Result<(), String> {
    svm.expire_blockhash();
    let mut all: Vec<&Keypair> = vec![payer];
    all.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(ixs, Some(&payer.pubkey()), &all, svm.latest_blockhash());
    svm.send_transaction(tx).map(|_| ()).map_err(|e| format!("{:?}", e.err))
}

fn expect_err<T: std::fmt::Debug>(result: Result<T, String>, code: u32) {
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

// ------------------------------------------------------------ instructions

fn ix_initialize(program: &Pubkey, deployer: &Pubkey, mint: &Pubkey, beneficiary: &Pubkey, epoch_seconds: u64) -> Instruction {
    let mut data = vec![0u8];
    data.extend_from_slice(beneficiary.as_ref());
    data.extend_from_slice(&epoch_seconds.to_le_bytes());
    Instruction {
        program_id: *program,
        accounts: vec![
            AccountMeta::new(*deployer, true),
            AccountMeta::new(pda(&[b"config", mint.as_ref()], program), false),
            AccountMeta::new(*mint, false),
            AccountMeta::new(pda(&[b"vault", mint.as_ref()], program), false),
            AccountMeta::new_readonly(pda(&[b"vault_authority", mint.as_ref()], program), false),
            AccountMeta::new(pda(&[b"founder_vault", mint.as_ref()], program), false),
            AccountMeta::new_readonly(pda(&[b"founder_authority", mint.as_ref()], program), false),
            AccountMeta::new_readonly(SYSTEM, false),
            AccountMeta::new_readonly(TOKEN, false),
        ],
        data,
    }
}

fn ix_create_epoch(env: &Env, signer: &Pubkey, e: u64) -> Instruction {
    let mut data = vec![1u8];
    data.extend_from_slice(&e.to_le_bytes());
    data.extend_from_slice(&[0xAB; 32]);
    Instruction {
        program_id: env.program,
        accounts: vec![
            AccountMeta::new(*signer, true),
            AccountMeta::new_readonly(env.config(), false),
            AccountMeta::new(env.epoch(e), false),
            AccountMeta::new_readonly(SYSTEM, false),
        ],
        data,
    }
}

fn ix_close(env: &Env, signer: &Pubkey, e: u64, head: [u8; 32]) -> Instruction {
    let mut data = vec![2u8];
    data.extend_from_slice(&e.to_le_bytes());
    data.extend_from_slice(&head);
    data.extend_from_slice(&1000u64.to_le_bytes());
    Instruction {
        program_id: env.program,
        accounts: vec![
            AccountMeta::new_readonly(*signer, true),
            AccountMeta::new(env.config(), false),
            AccountMeta::new(env.epoch(e), false),
            AccountMeta::new_readonly(env.vault(), false),
        ],
        data,
    }
}

fn result_data(tag: u8, e: u64, root: [u8; 32], total: u64, head: [u8; 32]) -> Vec<u8> {
    let mut data = vec![tag];
    data.extend_from_slice(&e.to_le_bytes());
    data.extend_from_slice(&root);
    data.extend_from_slice(&total.to_le_bytes());
    data.extend_from_slice(&head);
    data.extend_from_slice(&[0xEE; 32]);
    data
}

fn admin_epoch_ix(env: &Env, signer: &Pubkey, e: u64, data: Vec<u8>) -> Instruction {
    Instruction {
        program_id: env.program,
        accounts: vec![AccountMeta::new_readonly(*signer, true), AccountMeta::new_readonly(env.config(), false), AccountMeta::new(env.epoch(e), false)],
        data,
    }
}

fn ix_publish(env: &Env, signer: &Pubkey, e: u64, root: [u8; 32], total: u64) -> Instruction {
    admin_epoch_ix(env, signer, e, result_data(3, e, root, total, HEAD))
}

fn ix_republish(env: &Env, signer: &Pubkey, e: u64, root: [u8; 32], total: u64) -> Instruction {
    admin_epoch_ix(env, signer, e, result_data(5, e, root, total, HEAD))
}

fn ix_withdraw(env: &Env, signer: &Pubkey, e: u64) -> Instruction {
    let mut data = vec![4u8];
    data.extend_from_slice(&e.to_le_bytes());
    admin_epoch_ix(env, signer, e, data)
}

fn permissionless(env: &Env, tag: u8, e: u64) -> Instruction {
    let mut data = vec![tag];
    data.extend_from_slice(&e.to_le_bytes());
    Instruction { program_id: env.program, accounts: vec![AccountMeta::new(env.config(), false), AccountMeta::new(env.epoch(e), false)], data }
}

fn ix_expire(env: &Env, e: u64) -> Instruction {
    permissionless(env, 6, e)
}

fn ix_finalize(env: &Env, e: u64) -> Instruction {
    permissionless(env, 7, e)
}

fn ix_claim(env: &Env, claimant: &Pubkey, e: u64, amount: u64, proof: &[[u8; 32]], destination: &Pubkey) -> Instruction {
    let mut data = vec![8u8];
    data.extend_from_slice(&e.to_le_bytes());
    data.extend_from_slice(&amount.to_le_bytes());
    data.push(proof.len() as u8);
    for p in proof {
        data.extend_from_slice(p);
    }
    let ep = env.epoch(e);
    Instruction {
        program_id: env.program,
        accounts: vec![
            AccountMeta::new(*claimant, true),
            AccountMeta::new(env.config(), false),
            AccountMeta::new(ep, false),
            AccountMeta::new(pda(&[b"claim", ep.as_ref(), claimant.as_ref()], &env.program), false),
            AccountMeta::new(env.vault(), false),
            AccountMeta::new(*destination, false),
            AccountMeta::new_readonly(env.vault_authority(), false),
            AccountMeta::new_readonly(TOKEN, false),
            AccountMeta::new_readonly(SYSTEM, false),
        ],
        data,
    }
}

fn ix_set_admin(env: &Env, signer: &Pubkey, new_admin: &Pubkey) -> Instruction {
    let mut data = vec![9u8];
    data.extend_from_slice(new_admin.as_ref());
    Instruction { program_id: env.program, accounts: vec![AccountMeta::new_readonly(*signer, true), AccountMeta::new(env.config(), false)], data }
}

fn ix_fund_bounty(env: &Env, funder: &Pubkey, funder_ata: &Pubkey, id: u64, amount: u64) -> Instruction {
    let mut data = vec![10u8];
    data.extend_from_slice(&id.to_le_bytes());
    data.extend_from_slice(&amount.to_le_bytes());
    Instruction {
        program_id: env.program,
        accounts: vec![
            AccountMeta::new(*funder, true),
            AccountMeta::new(*funder_ata, false),
            AccountMeta::new(env.config(), false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new(env.vault(), false),
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
        data: vec![11],
    }
}

fn ix_release(env: &Env, beneficiary: &Pubkey, destination: &Pubkey) -> Instruction {
    Instruction {
        program_id: env.program,
        accounts: vec![
            AccountMeta::new_readonly(*beneficiary, true),
            AccountMeta::new(env.config(), false),
            AccountMeta::new(env.founder_vault(), false),
            AccountMeta::new_readonly(env.founder_authority(), false),
            AccountMeta::new(*destination, false),
            AccountMeta::new_readonly(TOKEN, false),
        ],
        data: vec![12],
    }
}

// ------------------------------------------------------------------ fixtures

fn base(devnet: bool) -> (LiteSVM, Pubkey, Keypair) {
    let mut svm = LiteSVM::new();
    let program = Pubkey::new_unique();
    svm.add_program_from_file(program, so_path(devnet)).expect("build the program with `cargo build-sbf` first (see header)");
    let admin = Keypair::new();
    svm.airdrop(&admin.pubkey(), 100_000_000_000_000).unwrap();
    let mut clock: solana_clock::Clock = svm.get_sysvar();
    clock.unix_timestamp = T0;
    svm.set_sysvar(&clock);
    (svm, program, admin)
}

fn genesis_with(devnet: bool, epoch_seconds: i64) -> Env {
    let (mut svm, program, admin) = base(devnet);
    let founder = Keypair::new();
    svm.airdrop(&founder.pubkey(), 100_000_000_000).unwrap();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 100_000_000_000_000).unwrap();
    let mint = create_mint(&mut svm, &admin, &admin.pubkey(), None, 6);
    let founder_ata = token_account(&mut svm, &admin, &mint, &founder.pubkey());
    send(&mut svm, &[ix_initialize(&program, &admin.pubkey(), &mint, &founder.pubkey(), epoch_seconds as u64)], &admin, &[]).unwrap();
    let env = Env {
        svm,
        program,
        admin,
        founder,
        founder_ata,
        mint,
        payer,
        epoch_seconds,
        m: Model { v: MINING, ..Default::default() },
        wallets: vec![founder_ata],
        escrows: vec![],
        winners: BTreeMap::new(),
    };
    env.check();
    env
}

fn genesis() -> Env {
    genesis_with(false, DAY)
}

fn funded(env: &mut Env) -> Keypair {
    let k = Keypair::new();
    env.svm.airdrop(&k.pubkey(), 10_000_000_000).unwrap();
    k
}

const G2: [u64; 10] = [
    34_176_150_000,
    34_163_172_119,
    34_150_199_167,
    34_137_231_141,
    34_124_268_040,
    34_111_309_861,
    34_098_356_602,
    34_085_408_263,
    34_072_464_841,
    34_059_526_333,
];

// =========================================================== G1: genesis

#[test]
fn g1_genesis_100m_10m_vesting_90m_mining_mint_dead() {
    let env = genesis();
    let (authority, supply, freeze) = mint_state(&env.svm, &env.mint);
    assert_eq!((authority, supply, freeze), (None, MAX, None));
    assert_eq!(balance(&env.svm, &env.founder_vault()), FOUNDER);
    assert_eq!(balance(&env.svm, &env.vault()), MINING);
    assert_eq!(balance(&env.svm, &env.founder_ata), 0, "founder wallet gets nothing at genesis");
    let c = env.chain();
    assert_eq!((c.p, c.o, c.last_closed, c.genesis, c.epoch_seconds), (0, 0, u64::MAX, T0, DAY));
    assert_eq!(c.beneficiary, env.founder.pubkey());
    assert_eq!(c.admin, env.admin.pubkey());
    // Vaults are token accounts owned by distinct PDAs.
    let v = env.svm.get_account(&env.vault()).unwrap().data;
    assert_eq!(Pubkey::new_from_array(v[32..64].try_into().unwrap()), env.vault_authority());
    let f = env.svm.get_account(&env.founder_vault()).unwrap().data;
    assert_eq!(Pubkey::new_from_array(f[32..64].try_into().unwrap()), env.founder_authority());
    assert_ne!(env.vault_authority(), env.founder_authority());
    assert_eq!(env.avail(), MINING);
}

#[test]
fn mint_authority_death_test() {
    let mut env = genesis();
    let attacker = funded(&mut env);
    let target = env.founder_ata;
    for signer in [env.founder.insecure_clone(), env.admin.insecure_clone(), attacker] {
        let ix = token_ix(7, 1, vec![AccountMeta::new(env.mint, false), AccountMeta::new(target, false), AccountMeta::new_readonly(signer.pubkey(), true)]);
        assert!(env.send(&[ix], &signer, &[]).is_err());
    }
    let admin = env.admin();
    for pda_key in [env.vault_authority(), env.founder_authority(), env.config()] {
        let ix = token_ix(7, 1, vec![AccountMeta::new(env.mint, false), AccountMeta::new(target, false), AccountMeta::new_readonly(pda_key, false)]);
        assert!(env.send(&[ix], &admin, &[]).is_err());
    }
    let mut data = vec![6u8, 0u8, 1u8];
    data.extend_from_slice(admin.pubkey().as_ref());
    let ix = Instruction { program_id: TOKEN, accounts: vec![AccountMeta::new(env.mint, false), AccountMeta::new_readonly(admin.pubkey(), true)], data };
    assert!(env.send(&[ix], &admin, &[]).is_err());
    env.check();
}

#[test]
fn genesis_rejects_bad_mints_wrong_signer_duplicates_and_bad_epoch() {
    let (mut svm, program, admin) = base(false);
    let founder = Keypair::new();
    let mint = create_mint(&mut svm, &admin, &admin.pubkey(), None, 6);
    send(&mut svm, &[ix_initialize(&program, &admin.pubkey(), &mint, &founder.pubkey(), DAY as u64)], &admin, &[]).unwrap();
    expect_err(send(&mut svm, &[ix_initialize(&program, &admin.pubkey(), &mint, &founder.pubkey(), DAY as u64)], &admin, &[]), ALREADY_INITIALIZED);
    let frozen = create_mint(&mut svm, &admin, &admin.pubkey(), Some(&admin.pubkey()), 6);
    expect_err(send(&mut svm, &[ix_initialize(&program, &admin.pubkey(), &frozen, &founder.pubkey(), DAY as u64)], &admin, &[]), BAD_MINT);
    let nine = create_mint(&mut svm, &admin, &admin.pubkey(), None, 9);
    expect_err(send(&mut svm, &[ix_initialize(&program, &admin.pubkey(), &nine, &founder.pubkey(), DAY as u64)], &admin, &[]), BAD_MINT);
    let intruder = Keypair::new();
    svm.airdrop(&intruder.pubkey(), 1_000_000_000).unwrap();
    let fresh = create_mint(&mut svm, &admin, &admin.pubkey(), None, 6);
    expect_err(send(&mut svm, &[ix_initialize(&program, &intruder.pubkey(), &fresh, &founder.pubkey(), DAY as u64)], &intruder, &[]), BAD_MINT);
    let pre = create_mint(&mut svm, &admin, &admin.pubkey(), None, 6);
    let p_ata = token_account(&mut svm, &admin, &pre, &founder.pubkey());
    send(&mut svm, &[token_ix(7, 5, vec![AccountMeta::new(pre, false), AccountMeta::new(p_ata, false), AccountMeta::new_readonly(admin.pubkey(), true)])], &admin, &[]).unwrap();
    expect_err(send(&mut svm, &[ix_initialize(&program, &admin.pubkey(), &pre, &founder.pubkey(), DAY as u64)], &admin, &[]), BAD_MINT);
    // The PRODUCTION binary accepts only 24 h epochs (no fast clock => no vesting bypass).
    let m2 = create_mint(&mut svm, &admin, &admin.pubkey(), None, 6);
    for bad in [24u64, 0, 86_388, 86_401, u64::MAX] {
        expect_err(send(&mut svm, &[ix_initialize(&program, &admin.pubkey(), &m2, &founder.pubkey(), bad)], &admin, &[]), BAD_EPOCH_SECONDS);
    }
    // Zero beneficiary is refused.
    expect_err(send(&mut svm, &[ix_initialize(&program, &admin.pubkey(), &m2, &Pubkey::default(), DAY as u64)], &admin, &[]), 1);
}

// ================================================ G2–G4: caps and half-life

#[test]
fn g2_first_ten_caps_realistic_parallel_cadence() {
    let mut env = genesis();
    let caps = env.cadence(0..10, &[], &|c| c);
    assert_eq!(caps, G2.to_vec(), "R1: parallel holds give exactly the sequential vectors");
    env.drain();
    assert_eq!(env.avail(), MINING - G2.iter().sum::<u64>());
}

#[test]
fn g3_g4_hundred_and_1826_successful_epochs_half_life() {
    let mut env = genesis();
    env.cadence(0..100, &[], &|c| c);
    env.set_time(env.close_time(100));
    env.settle_due();
    assert_eq!(env.avail(), 86_645_835_909_879, "G3: A before close of epoch 100");
    assert_eq!(env.try_close(100).unwrap(), 32_902_456_499, "G3 next cap");
    env.set_time(env.now() + 3_600);
    env.try_publish(100, 32_902_456_499, false).unwrap();
    env.cadence(101..1825, &[], &|c| c);
    env.set_time(env.close_time(1825));
    env.settle_due();
    let a1825 = env.avail();
    assert_eq!(a1825, 44_999_963_593_560);
    assert!(a1825 <= MINING / 2, "half-life reached after exactly 1,825 successful epochs");
    env.try_close(1825).unwrap();
    env.set_time(env.now() + 3_600);
    let cap = env.m.epochs[&1825].cap;
    env.try_publish(1825, cap, false).unwrap();
    env.set_time(env.close_time(1826));
    env.settle_due();
    assert_eq!(env.avail(), 44_982_875_532_385, "G4 A after 1,826");
    assert_eq!(env.try_close(1826).unwrap(), 17_081_572_240, "G4 next cap");
}

#[test]
fn half_life_not_reached_after_1824() {
    let mut a = MINING;
    for _ in 0..1_824 {
        a -= cap_of(a);
    }
    assert!(a > MINING / 2);
}

// ============================================================ G5: downtime

#[test]
fn g5_thirty_and_365_days_of_downtime_no_catch_up() {
    for gap in [30u64, 365] {
        let mut env = genesis();
        env.cadence(0..10, &[], &|c| c);
        env.drain();
        assert_eq!(env.close(10 + gap), 34_046_592_739, "gap {gap}");
    }
}

// ================================================ G6–G8: failed / no winner

#[test]
fn g6_failed_epoch_expires_and_releases_its_cap() {
    let mut env = genesis();
    env.close(0);
    env.try_publish(0, G2[0], false).unwrap();
    assert_eq!(env.close(1), 34_163_172_119);
    // epoch 1 is never published; its deadline is close + 48 h
    let deadline = env.m.epochs[&1].deadline;
    env.set_time(deadline - 1);
    expect_err(env.try_expire(1), DEADLINE_NOT_REACHED);
    env.set_time(deadline);
    expect_err(env.try_publish(1, 1, false), DEADLINE_PASSED);
    env.try_expire(1).unwrap();
    expect_err(env.try_expire(1), WRONG_PHASE);
    expect_err(env.try_publish(1, 0, false), DEADLINE_PASSED);
    expect_err(env.try_finalize(1), WRONG_PHASE);
    env.drain();
    assert_eq!(env.close(3), 34_163_172_119, "G6: next cap identical, no decay from the failure");
}

#[test]
fn g7_no_winner_epoch_publishes_zero_and_releases() {
    let mut env = genesis();
    env.close(0);
    env.try_publish(0, G2[0], false).unwrap();
    assert_eq!(env.close(1), 34_163_172_119);
    env.try_publish(1, 0, false).unwrap();
    env.drain();
    assert_eq!(env.m.epochs[&1].status, FINAL);
    assert_eq!(env.close(5), 34_163_172_119, "G7");
}

#[test]
fn g8_thousand_no_winner_epochs() {
    let mut env = genesis();
    env.cadence(0..1000, &[], &|_| 0);
    env.drain();
    assert_eq!(env.avail(), MINING, "no permanent depletion at all");
    assert_eq!(env.chain().awarded, 0);
    assert_eq!(env.close(1003), 34_176_150_000, "G8");
}

// ============================================================ G9: bounty

#[test]
fn g9_bounty_recycling_95_5_and_rounding() {
    let mut env = genesis();
    let admin = env.admin();
    // founder needs liquid tokens: release at 2 years
    env.set_time(T0 + 2 * YEAR);
    assert_eq!(env.try_release().unwrap(), 2_500_000_000_000);
    assert_eq!(env.try_fund_bounty(1, 1_000_000_000).unwrap(), (50_000_000, 950_000_000));
    assert_eq!(env.try_fund_bounty(2, 1_000_000_019).unwrap(), (50_000_000, 950_000_019));
    assert_eq!(balance(&env.svm, &env.escrow(1)), 950_000_000);
    assert_eq!(balance(&env.svm, &env.escrow(2)), 950_000_019);
    let f = env.founder.insecure_clone();
    let ata = env.founder_ata;
    expect_err(env.send(&[ix_fund_bounty(&env, &f.pubkey(), &ata, 3, 999_999)], &f, &[]), BOUNTY_TOO_SMALL);
    expect_err(env.send(&[ix_fund_bounty(&env, &f.pubkey(), &ata, 3, 0)], &f, &[]), BOUNTY_TOO_SMALL);
    expect_err(env.send(&[ix_fund_bounty(&env, &f.pubkey(), &ata, 1, 1_000_000_000)], &f, &[]), ALREADY_INITIALIZED);
    assert_eq!(mint_state(&env.svm, &env.mint).1, MAX, "no burn");
    // Award once, admin only.
    let solver = funded(&mut env);
    let sa = env.wallet_for(&solver);
    expect_err(env.send(&[ix_award_bounty(&env, &solver.pubkey(), 1, &sa)], &solver, &[]), NOT_ADMIN);
    env.send(&[ix_award_bounty(&env, &admin.pubkey(), 1, &sa)], &admin, &[]).unwrap();
    env.m.escrow -= 950_000_000;
    env.check();
    expect_err(env.send(&[ix_award_bounty(&env, &admin.pubkey(), 1, &sa)], &admin, &[]), BOUNTY_CLOSED);
    assert_eq!(balance(&env.svm, &sa), 950_000_000);
}

#[test]
fn g9_fee_does_not_change_a_pinned_cap_and_raises_the_next() {
    let mut env = genesis();
    env.set_time(T0 + 2 * YEAR);
    env.try_release().unwrap();
    let base_e = 730u64; // epoch 730 starts at T0 + 2 years
    env.close(base_e);
    env.try_publish(base_e, G2[0], false).unwrap();
    env.drain();
    let k = env.close(base_e + 4);
    assert_eq!(k, 34_163_172_119);
    env.try_fund_bounty(1, 1_000_000_000).unwrap();
    assert_eq!(env.chain_epoch(base_e + 4).cap, 34_163_172_119, "pinned cap unchanged by the fee");
    env.try_publish(base_e + 4, k, false).unwrap();
    env.drain();
    assert_eq!(env.close(base_e + 8), 34_150_218_154, "G9: next cap includes the recycled fee");
}

// =================================================== G10: outstanding claims

#[test]
fn g10_ten_outstanding_and_parallel_claims_do_not_change_a() {
    let mut env = genesis();
    let mut caps = Vec::new();
    for e in 0..10u64 {
        caps.push(env.close(e));
        env.set_time(env.now() + 60);
        env.try_publish(e, caps[e as usize], false).unwrap();
    }
    assert_eq!(caps, G2.to_vec());
    // epochs 10..12 close while 0..9 are still reserved/outstanding
    let next: Vec<u64> = (10..13).map(|e| env.close(e)).collect();
    assert_eq!(next, vec![34_046_592_739, 34_033_664_056, 34_020_740_283]);
    // finalize 0..9 (outstanding, unclaimed)
    env.set_time(env.m.epochs[&9].open_ts);
    for e in 0..10 {
        env.try_finalize(e).unwrap();
    }
    assert_eq!(env.chain().o, 341_178_086_367, "G10 O");
    let a0 = env.avail();
    assert_eq!(a0, 89_556_720_916_555);
    for e in 0..10 {
        env.try_claim_winner(e).unwrap();
        assert_eq!(env.avail(), a0, "claims never change A");
    }
    assert_eq!(balance(&env.svm, &env.vault()), 89_658_821_913_633, "G10 V");
    assert_eq!(env.chain().o, 0);
}

// ================================================================= R1

#[test]
fn r1_failed_epoch_in_parallel_does_not_deplete() {
    let mut ok = genesis();
    let caps_ok = ok.cadence(0..12, &[], &|c| c);
    let mut bad = genesis();
    let caps_bad = bad.cadence(0..12, &[1], &|c| c);
    assert_eq!(caps_ok[..10], G2);
    assert_eq!(caps_bad[..6], [34_176_150_000, 34_163_172_119, 34_150_199_167, 34_150_204_093, 34_137_236_066, 34_124_272_962]);
    // drain both at the same instant
    let t = ok.close_time(20);
    ok.set_time(t);
    bad.set_time(t);
    ok.drain();
    bad.drain();
    assert_eq!(ok.avail(), 89_590_741_656_838);
    assert_eq!(bad.avail(), 89_624_788_249_578);
    assert_eq!(bad.avail() - ok.avail(), 34_046_592_740, "the failed epoch released its whole cap");
    assert_eq!(bad.chain().awarded, 375_211_750_422);
    assert_eq!(ok.chain().awarded, 409_258_343_162);
    assert_eq!(bad.avail(), MINING - bad.chain().awarded, "A = MINING - awarded once nothing is held");
}

#[test]
fn r1_all_zero_and_partial_awards() {
    let mut z = genesis();
    z.cadence(0..12, &[], &|_| 0);
    z.drain();
    assert_eq!(z.avail(), MINING);
    let mut h = genesis();
    h.cadence(0..12, &[], &|c| c / 2);
    h.drain();
    assert_eq!(h.avail(), MINING - h.chain().awarded);
    // the unused half of each cap was released, not consumed
    let caps: u64 = h.m.epochs.values().map(|m| m.cap).sum();
    assert!(h.chain().awarded < caps);
}

#[test]
fn r1_no_double_reservation_hold_until_final() {
    let mut env = genesis();
    let k0 = env.close(0);
    env.try_publish(0, k0 / 3, false).unwrap();
    // publish does not release anything: the whole cap is still held
    assert_eq!(env.chain().p, k0);
    let k1 = env.close(1);
    assert_eq!(k1, cap_of(MINING - k0), "the parallel epoch excludes the WHOLE held cap");
    env.set_time(env.m.epochs[&0].open_ts);
    env.try_finalize(0).unwrap();
    assert_eq!(env.chain().p, k1);
    assert_eq!(env.chain().o, k0 / 3);
    expect_err(env.try_finalize(0), WRONG_PHASE);
    expect_err(env.try_expire(0), WRONG_PHASE);
}

// ================================================================= R2

#[test]
fn r2_single_correction_state_machine() {
    let mut env = genesis();
    let admin = env.admin();
    let mallory = funded(&mut env);
    let k = env.close(0);
    let (p0, a0) = (env.chain().p, env.avail());
    let t = env.now() + 100;
    env.set_time(t);
    env.try_publish(0, k, false).unwrap();
    let open0 = env.m.epochs[&0].open_ts;
    assert_eq!((env.chain().p, env.avail()), (p0, a0));
    expect_err(env.try_claim_winner(0), NOT_FINAL);
    expect_err(env.send(&[ix_withdraw(&env, &mallory.pubkey(), 0)], &mallory, &[]), NOT_ADMIN);
    expect_err(env.try_publish(0, k, true), WRONG_PHASE); // republish needs a withdraw first
    expect_err(env.try_publish(0, k, false), ROOT_ALREADY_PUBLISHED);
    env.set_time(t + 100);
    env.try_withdraw(0).unwrap();
    assert_eq!((env.chain().p, env.chain().o, env.avail()), (p0, 0, a0), "no reservation change");
    let ce = env.chain_epoch(0);
    assert_eq!((ce.orig_total, ce.deadline, ce.status), (k, open0, WITHDRAWN), "original kept; deadline = original claim_open_ts");
    // original root kept on-chain
    let d = env.svm.get_account(&env.epoch(0)).unwrap().data;
    assert_ne!(&d[186..218], &[0u8; 32]);
    expect_err(env.try_withdraw(0), CORRECTION_USED);
    env.set_time(t + 200);
    expect_err(env.try_publish(0, k + 1, true), EPOCH_CAP_EXCEEDED);
    let a = env.admin();
    let mut wrong_head = ix_republish(&env, &a.pubkey(), 0, [1; 32], 1);
    wrong_head.data[1 + 8 + 32 + 8..1 + 8 + 32 + 8 + 32].copy_from_slice(&[0x99; 32]);
    expect_err(env.send(&[wrong_head], &a, &[]), LOG_HEAD_MISMATCH);
    expect_err(env.send(&[ix_republish(&env, &mallory.pubkey(), 0, [1; 32], 1)], &mallory, &[]), NOT_ADMIN);
    env.try_publish(0, k - 1, true).unwrap();
    let z = env.chain_epoch(0);
    assert_eq!((z.status, z.corrections, z.total, z.orig_total), (CORRECTED, 1, k - 1, k));
    assert!(z.open_ts - (t) <= 2 * CLAIM_DELAY, "bounded: < publish + 2*CLAIM_DELAY");
    assert_eq!(z.open_ts, t + 200 + CLAIM_DELAY);
    expect_err(env.try_withdraw(0), CORRECTION_USED);
    expect_err(env.try_publish(0, 1, true), CORRECTION_USED);
    expect_err(env.try_publish(0, 1, false), ROOT_ALREADY_PUBLISHED);
    env.set_time(z.open_ts - 1);
    expect_err(env.try_finalize(0), CLAIM_WINDOW_NOT_OPEN);
    env.set_time(z.open_ts);
    env.try_finalize(0).unwrap();
    env.try_claim_winner(0).unwrap();
    expect_err(env.try_withdraw(0), CORRECTION_USED);
    let _ = admin;
    // claimed result can never be withdrawn; next cap reflects k-1 awarded
    assert_eq!(env.close(4), cap_of(MINING - (k - 1)));
    assert_eq!(cap_of(MINING - (k - 1)), 34_163_172_119);
}

#[test]
fn r2_withdraw_only_inside_window_and_deadline_releases_reservation() {
    // withdraw at/after claim_open_ts is refused (result already claimable)
    let mut env = genesis();
    let k = env.close(0);
    env.try_publish(0, k, false).unwrap();
    let open = env.m.epochs[&0].open_ts;
    env.set_time(open);
    expect_err(env.try_withdraw(0), DEADLINE_PASSED);
    env.try_finalize(0).unwrap();
    expect_err(env.try_withdraw(0), WRONG_PHASE);

    // republish deadline = original claim_open_ts; afterwards expire releases the cap
    let mut env = genesis();
    let k = env.close(0);
    let t = env.now();
    env.try_publish(0, k, false).unwrap();
    env.set_time(t + CLAIM_DELAY - 10);
    env.try_withdraw(0).unwrap();
    env.set_time(t + CLAIM_DELAY);
    expect_err(env.try_publish(0, k, true), DEADLINE_PASSED);
    env.set_time(t + CLAIM_DELAY - 1);
    expect_err(env.try_expire(0), DEADLINE_NOT_REACHED);
    env.set_time(t + CLAIM_DELAY);
    env.try_expire(0).unwrap();
    assert_eq!((env.chain().p, env.chain().o, env.avail()), (0, 0, MINING));
    env.set_time(t + CLAIM_DELAY + 1);
    expect_err(env.try_publish(0, k, true), DEADLINE_PASSED);
    expect_err(env.try_finalize(0), WRONG_PHASE);
    expect_err(env.try_withdraw(0), CORRECTION_USED);
    assert_eq!(env.close(3), 34_176_150_000, "R2 after expire: no depletion");
}

#[test]
fn r2_correction_does_not_touch_parallel_caps_and_zero_republish() {
    let mut env = genesis();
    let k0 = env.close(0);
    env.try_publish(0, k0, false).unwrap();
    let k1 = env.close(1);
    env.set_time(env.now() + 5);
    env.try_withdraw(0).unwrap();
    let a = env.avail();
    env.set_time(env.now() + 1);
    env.try_publish(0, 1, true).unwrap();
    assert_eq!(env.avail(), a);
    let k2 = env.close(2);
    assert_eq!((k0, k1, k2), (34_176_150_000, 34_163_172_119, 34_150_199_167));

    let mut env = genesis();
    let k = env.close(0);
    env.try_publish(0, k, false).unwrap();
    env.set_time(env.now() + 1);
    env.try_withdraw(0).unwrap();
    env.set_time(env.now() + 1);
    env.try_publish(0, 0, true).unwrap();
    env.drain();
    assert_eq!(env.avail(), MINING);
}

// ==================================================== state-machine guards

#[test]
fn close_window_order_and_repeated_close() {
    let mut env = genesis();
    let admin = env.admin();
    let mallory = funded(&mut env);
    env.try_create(0).unwrap();
    expect_err(env.try_create(0), EPOCH_EXISTS);
    expect_err(env.send(&[ix_create_epoch(&env, &mallory.pubkey(), 9)], &mallory, &[]), NOT_ADMIN);
    // exact window boundaries [s + 22h, s + 30h)
    env.set_time(T0 + REVEAL_END - 1);
    expect_err(env.try_close(0), OUTSIDE_CLOSE_WINDOW);
    expect_err(env.send(&[ix_close(&env, &mallory.pubkey(), 0, HEAD)], &mallory, &[]), NOT_ADMIN);
    env.set_time(T0 + DAY + CLOSE_GRACE);
    expect_err(env.try_close(0), OUTSIDE_CLOSE_WINDOW);
    env.set_time(T0 + DAY + CLOSE_GRACE - 1);
    env.try_close(0).unwrap();
    expect_err(env.try_close(0), WRONG_PHASE);
    // publish before close / wrong head
    env.try_create(5).unwrap();
    expect_err(env.try_publish(5, 0, false), WRONG_PHASE);
    let mut wrong = ix_publish(&env, &admin.pubkey(), 0, [1; 32], 1);
    wrong.data[1 + 8 + 32 + 8..1 + 8 + 32 + 8 + 32].copy_from_slice(&[0x99; 32]);
    expect_err(env.send(&[wrong], &admin, &[]), LOG_HEAD_MISMATCH);
    expect_err(env.try_publish(0, env.m.epochs[&0].cap + 1, false), EPOCH_CAP_EXCEEDED);
    // older or equal index after a later close is refused
    env.close(3);
    env.try_create(2).unwrap();
    env.set_time(env.close_time(3) + 1);
    expect_err(env.try_close(2), EPOCH_ORDER);
    expect_err(env.try_close(3), WRONG_PHASE);
    // the wrong epoch account for an index
    let mut ix = ix_close(&env, &admin.pubkey(), 4, HEAD);
    ix.accounts[2] = AccountMeta::new(env.epoch(5), false);
    expect_err(env.send(&[ix], &admin, &[]), BAD_ACCOUNT);
    // substituted vault at close
    let fake = env.founder_ata;
    let mut ix = ix_close(&env, &admin.pubkey(), 5, HEAD);
    ix.accounts[3] = AccountMeta::new_readonly(fake, false);
    expect_err(env.send(&[ix], &admin, &[]), BAD_PDA);
    env.check();
}

#[test]
fn reserve_underflow_aborts_close() {
    let mut env = genesis();
    env.close(0);
    // Tamper (test-only) so that V < P: close must abort, never wrap.
    let mut acct = env.svm.get_account(&env.vault()).unwrap();
    let p = env.chain().p;
    acct.data[64..72].copy_from_slice(&(p - 1).to_le_bytes());
    env.svm.set_account(env.vault(), acct).unwrap();
    env.try_create(1).unwrap();
    env.set_time(env.close_time(1));
    expect_err(env.try_close(1), RESERVE_UNDERFLOW);
}

#[test]
fn claims_once_only_from_final_and_never_above_total() {
    let mut env = genesis();
    let alice = funded(&mut env);
    let bob = funded(&mut env);
    let k = env.close(0);
    // buggy tree sums above the published total
    let leaves = [leaf(0, &alice.pubkey(), 600), leaf(0, &bob.pubkey(), 600)];
    let (root, proofs) = tree(&leaves);
    let a = env.admin();
    env.send(&[ix_publish(&env, &a.pubkey(), 0, root, 1_000)], &a, &[]).unwrap();
    let me = env.m.epochs.get_mut(&0).unwrap();
    me.status = PUBLISHED;
    me.total = 1_000;
    me.deadline = 0;
    me.open_ts = env.svm.get_sysvar::<solana_clock::Clock>().unix_timestamp + CLAIM_DELAY;
    assert!(k > 1_000);
    env.check();
    let aa = env.wallet_for(&alice);
    let ba = env.wallet_for(&bob);
    let claim_a = ix_claim(&env, &alice.pubkey(), 0, 600, &proofs[0], &aa);
    expect_err(env.send(&[claim_a.clone()], &alice, &[]), NOT_FINAL);
    env.set_time(env.m.epochs[&0].open_ts);
    // finalize + claim in one transaction
    let p = env.payer.insecure_clone();
    env.send(&[ix_finalize(&env, 0), claim_a.clone()], &alice, &[]).unwrap();
    let me = env.m.epochs.get_mut(&0).unwrap();
    me.status = FINAL;
    me.claimed = 600;
    env.m.p -= k;
    env.m.o += 400;
    env.m.awarded += 1_000;
    env.m.v -= 600;
    env.m.claimed_total += 600;
    env.check();
    expect_err(env.send(&[claim_a], &alice, &[]), ALREADY_CLAIMED);
    expect_err(env.send(&[ix_claim(&env, &bob.pubkey(), 0, 600, &proofs[1], &ba)], &bob, &[]), CAP_EXCEEDED);
    // forged amount / wrong wallet / wrong epoch
    expect_err(env.send(&[ix_claim(&env, &bob.pubkey(), 0, 400, &proofs[1], &ba)], &bob, &[]), INVALID_PROOF);
    let mallory = funded(&mut env);
    let ma = env.wallet_for(&mallory);
    expect_err(env.send(&[ix_claim(&env, &mallory.pubkey(), 0, 600, &proofs[1], &ma)], &mallory, &[]), INVALID_PROOF);
    // destination not owned by the claimant (redirect) is refused
    expect_err(env.send(&[ix_claim(&env, &bob.pubkey(), 0, 600, &proofs[1], &ma)], &bob, &[]), BAD_ACCOUNT);
    // substituted vault / vault authority
    let mut ix = ix_claim(&env, &bob.pubkey(), 0, 600, &proofs[1], &ba);
    ix.accounts[4] = AccountMeta::new(ma, false);
    expect_err(env.send(&[ix], &bob, &[]), BAD_PDA);
    let mut ix = ix_claim(&env, &bob.pubkey(), 0, 600, &proofs[1], &ba);
    ix.accounts[6] = AccountMeta::new_readonly(mallory.pubkey(), false);
    expect_err(env.send(&[ix], &bob, &[]), BAD_PDA);
    // claim on an epoch account of another index
    let mut ix = ix_claim(&env, &bob.pubkey(), 1, 600, &proofs[1], &ba);
    ix.accounts[2] = AccountMeta::new(env.epoch(0), false);
    expect_err(env.send(&[ix], &bob, &[]), BAD_ACCOUNT);
    let _ = p;
    env.check();
}

#[test]
fn nobody_can_withdraw_the_reserve_or_founder_vault() {
    let mut env = genesis();
    let attacker = funded(&mut env);
    let sink = env.wallet_for(&attacker);
    for signer in [env.founder.insecure_clone(), env.admin.insecure_clone(), attacker.insecure_clone()] {
        for src in [env.vault(), env.founder_vault()] {
            let ix = token_ix(3, 1, vec![AccountMeta::new(src, false), AccountMeta::new(sink, false), AccountMeta::new_readonly(signer.pubkey(), true)]);
            assert!(env.send(&[ix], &signer, &[]).is_err());
            let ix = token_ix(8, 1, vec![AccountMeta::new(src, false), AccountMeta::new(env.mint, false), AccountMeta::new_readonly(signer.pubkey(), true)]);
            assert!(env.send(&[ix], &signer, &[]).is_err(), "burn");
        }
    }
    // no instruction tag beyond 12 exists
    for tag in [13u8, 99, 255] {
        let admin = env.admin();
        let ix = Instruction { program_id: env.program, accounts: vec![AccountMeta::new(env.config(), false)], data: vec![tag] };
        assert!(env.send(&[ix], &admin, &[]).is_err());
    }
    env.check();
}

// ================================================================= vesting

#[test]
fn vesting_cliff_linear_end_and_only_the_beneficiary() {
    let mut env = genesis();
    let admin = env.admin();
    let mallory = funded(&mut env);
    let ma = env.wallet_for(&mallory);
    expect_err(env.try_release(), NOTHING_VESTED);
    env.set_time(T0 + YEAR - 1);
    expect_err(env.try_release(), NOTHING_VESTED);
    env.set_time(T0 + YEAR);
    expect_err(env.try_release(), NOTHING_VESTED);
    env.set_time(T0 + YEAR + 1);
    assert_eq!(env.try_release().unwrap(), 79_274);
    // admin / attacker cannot release; no redirect to another owner's account
    let fata = env.founder_ata;
    expect_err(env.send(&[ix_release(&env, &admin.pubkey(), &fata)], &admin, &[]), NOT_BENEFICIARY);
    expect_err(env.send(&[ix_release(&env, &mallory.pubkey(), &ma)], &mallory, &[]), NOT_BENEFICIARY);
    let f = env.founder.insecure_clone();
    env.set_time(T0 + 2 * YEAR);
    expect_err(env.send(&[ix_release(&env, &f.pubkey(), &ma)], &f, &[]), BAD_ACCOUNT);
    // substituted founder vault / authority
    let mut ix = ix_release(&env, &f.pubkey(), &fata);
    ix.accounts[2] = AccountMeta::new(env.vault(), false);
    expect_err(env.send(&[ix], &f, &[]), BAD_PDA);
    let mut ix = ix_release(&env, &f.pubkey(), &fata);
    ix.accounts[3] = AccountMeta::new_readonly(env.vault_authority(), false);
    expect_err(env.send(&[ix], &f, &[]), BAD_PDA);
    let mut ix = ix_release(&env, &f.pubkey(), &fata);
    ix.accounts[0].is_signer = false;
    assert!(env.send(&[ix], &admin, &[]).is_err());
    assert_eq!(env.try_release().unwrap(), 2_500_000_000_000 - 79_274);
    expect_err(env.try_release(), NOTHING_VESTED);
    env.set_time(T0 + 5 * YEAR - 1);
    assert_eq!(env.try_release().unwrap(), 9_999_999_920_725 - 2_500_000_000_000);
    env.set_time(T0 + 6 * YEAR);
    assert_eq!(env.try_release().unwrap(), FOUNDER - 9_999_999_920_725);
    env.set_time(T0 + 50 * YEAR);
    expect_err(env.try_release(), NOTHING_VESTED);
    assert_eq!(balance(&env.svm, &env.founder_vault()), 0);
    assert_eq!(balance(&env.svm, &env.founder_ata), FOUNDER, "never above the allocation");
    assert_eq!(env.chain().founder_claimed, FOUNDER);
    // the admin hand-off does not touch vesting
    let ms = funded(&mut env);
    env.send(&[ix_set_admin(&env, &admin.pubkey(), &ms.pubkey())], &admin, &[]).unwrap();
    assert_eq!(env.chain().beneficiary, env.founder.pubkey());
}

#[test]
fn g11_vesting_release_vector_2y_repeat_6y() {
    let mut env = genesis();
    env.set_time(T0 + 2 * YEAR);
    assert_eq!(env.try_release().unwrap(), 2_500_000_000_000);
    expect_err(env.try_release(), NOTHING_VESTED);
    env.set_time(T0 + 6 * YEAR);
    assert_eq!(env.try_release().unwrap(), 7_500_000_000_000);
    assert_eq!(env.chain().founder_claimed, FOUNDER);
}

// ============================================================ admin / pools

#[test]
fn admin_handoff_and_pool_claimants() {
    let mut env = genesis();
    let admin = env.admin();
    let ms = funded(&mut env);
    let mallory = funded(&mut env);
    expect_err(env.send(&[ix_set_admin(&env, &mallory.pubkey(), &mallory.pubkey())], &mallory, &[]), NOT_ADMIN);
    env.send(&[ix_set_admin(&env, &admin.pubkey(), &ms.pubkey())], &admin, &[]).unwrap();
    expect_err(env.send(&[ix_create_epoch(&env, &admin.pubkey(), 0)], &admin, &[]), NOT_ADMIN);
    env.admin = ms.insecure_clone();
    // pool + same-owner second wallet are ordinary claimants
    let pool = funded(&mut env);
    let w2 = funded(&mut env);
    let k = env.close(0);
    let (root, proofs) = tree(&[leaf(0, &pool.pubkey(), 2_000), leaf(0, &w2.pubkey(), 1_000)]);
    env.send(&[ix_publish(&env, &ms.pubkey(), 0, root, 3_000)], &ms, &[]).unwrap();
    let open = env.now() + CLAIM_DELAY;
    *env.m.epochs.get_mut(&0).unwrap() = MEpoch { status: PUBLISHED, cap: k, total: 3_000, open_ts: open, ..Default::default() };
    env.check();
    env.set_time(open);
    env.try_finalize(0).unwrap();
    let pa = env.wallet_for(&pool);
    let wa = env.wallet_for(&w2);
    // prefunded receipt address does not block the claim
    let receipt = pda(&[b"claim", env.epoch(0).as_ref(), pool.pubkey().as_ref()], &env.program);
    env.svm.set_account(receipt, Account { lamports: 1, data: vec![], owner: SYSTEM, executable: false, rent_epoch: 0 }).unwrap();
    env.send(&[ix_claim(&env, &pool.pubkey(), 0, 2_000, &proofs[0], &pa)], &pool, &[]).unwrap();
    env.send(&[ix_claim(&env, &w2.pubkey(), 0, 1_000, &proofs[1], &wa)], &w2, &[]).unwrap();
    let me = env.m.epochs.get_mut(&0).unwrap();
    me.claimed = 3_000;
    env.m.o -= 3_000;
    env.m.v -= 3_000;
    env.m.claimed_total += 3_000;
    env.check();
}

#[test]
fn python_and_rust_claim_leaves_agree() {
    let expected = include_str!("../../program/tests/vector_leaf.txt").trim();
    let got: String = leaf(0, &Pubkey::new_from_array([7; 32]), 1_000).iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(got, expected);
}

// ============================================== property / random transitions

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// Random interleavings of every transition (valid and invalid) against the
/// model; `check()` after each successful step asserts invariants 1–13.
/// Invalid attempts must fail and leave the state unchanged.
fn random_run(seed: u64, steps: usize) {
    let mut env = genesis();
    let mut rng = Rng(seed);
    let mut next_epoch = 0u64;
    let mut bounty_id = 0u64;
    let mut n = [0usize; 10]; // closes, publishes, withdraws, republishes, finalizes, expires, claims, releases, bounties, rejected
    for _ in 0..steps {
        let now = env.now();
        match rng.below(10) {
            0 | 1 => {
                // close the next epoch at a random point of its window or a bit outside
                let e = next_epoch + rng.below(2);
                let (open, end) = (env.close_time(e), env.start(e) + DAY + CLOSE_GRACE);
                let t = open - 100 + rng.below((end - open + 200) as u64) as i64;
                if t >= now {
                    env.set_time(t);
                    let res = env.try_close(e);
                    if t >= open && t < end {
                        res.unwrap();
                        n[0] += 1;
                        next_epoch = e + 1;
                    } else {
                        expect_err(res, OUTSIDE_CLOSE_WINDOW);
                        n[9] += 1;
                    }
                }
            }
            2 => {
                // publish (valid or above cap)
                if let Some((&e, me)) = env.m.epochs.iter().find(|(_, m)| m.status == CLOSED && now < m.deadline) {
                    let cap = me.cap;
                    if rng.below(4) == 0 {
                        expect_err(env.try_publish(e, cap + 1 + rng.below(1000), false), EPOCH_CAP_EXCEEDED);
                        n[9] += 1;
                    } else {
                        let total = match rng.below(3) {
                            0 => 0,
                            1 => cap,
                            _ => rng.below(cap + 1),
                        };
                        env.try_publish(e, total, false).unwrap();
                        n[1] += 1;
                    }
                }
            }
            3 => {
                // withdraw / republish
                let pick = env.m.epochs.iter().find(|(_, m)| matches!(m.status, PUBLISHED | WITHDRAWN | CORRECTED)).map(|(k, m)| (*k, m.clone()));
                if let Some((e, me)) = pick {
                    match me.status {
                        PUBLISHED if now < me.open_ts => {
                            env.try_withdraw(e).unwrap();
                            n[2] += 1;
                        }
                        PUBLISHED => expect_err(env.try_withdraw(e), DEADLINE_PASSED),
                        WITHDRAWN if now < me.deadline => {
                            let total = rng.below(me.cap + 1);
                            env.try_publish(e, total, true).unwrap();
                            n[3] += 1;
                        }
                        WITHDRAWN => expect_err(env.try_publish(e, 0, true), DEADLINE_PASSED),
                        _ => expect_err(env.try_withdraw(e), CORRECTION_USED),
                    }
                }
            }
            4 => {
                // finalize / expire whatever is due (or prove early attempts fail)
                let all: Vec<(u64, MEpoch)> = env.m.epochs.iter().map(|(k, v)| (*k, v.clone())).collect();
                for (e, me) in all {
                    match me.status {
                        PUBLISHED | CORRECTED if now >= me.open_ts => {
                            env.try_finalize(e).unwrap();
                            n[4] += 1;
                        }
                        PUBLISHED | CORRECTED => expect_err(env.try_finalize(e), CLAIM_WINDOW_NOT_OPEN),
                        CLOSED | WITHDRAWN if now >= me.deadline => {
                            env.try_expire(e).unwrap();
                            n[5] += 1;
                        }
                        CLOSED | WITHDRAWN => expect_err(env.try_expire(e), DEADLINE_NOT_REACHED),
                        FINAL | EXPIRED => {
                            expect_err(env.try_finalize(e), WRONG_PHASE);
                            expect_err(env.try_expire(e), WRONG_PHASE);
                        }
                        _ => {}
                    }
                }
            }
            5 => {
                // claims (double claim must fail)
                let claimable = |m: &MEpoch| m.status == FINAL && m.total > 0;
                let unclaimed = env.m.epochs.iter().find(|(k, m)| claimable(m) && m.claimed == 0 && env.winners.contains_key(k));
                let pick = if rng.below(4) == 0 { None } else { unclaimed }
                    .or_else(|| env.m.epochs.iter().find(|(k, m)| claimable(m) && env.winners.contains_key(k)))
                    .map(|(k, m)| (*k, m.clone()));
                if let Some((e, me)) = pick {
                    if me.claimed == 0 {
                        env.try_claim_winner(e).unwrap();
                        n[6] += 1;
                    } else {
                        expect_err(env.try_claim_winner(e), ALREADY_CLAIMED);
                        n[9] += 1;
                    }
                }
            }
            6 => {
                // vesting release
                let v = vested_at(T0, DAY, now);
                if v > env.m.founder_claimed {
                    env.try_release().unwrap();
                    n[7] += 1;
                } else {
                    expect_err(env.try_release(), NOTHING_VESTED);
                    n[9] += 1;
                }
            }
            7 => {
                // bounty (needs founder liquidity)
                let bal = balance(&env.svm, &env.founder_ata);
                if bal >= UNIT {
                    let amount = UNIT + rng.below(bal - UNIT + 1).min(10_000 * UNIT);
                    bounty_id += 1;
                    env.try_fund_bounty(bounty_id, amount).unwrap();
                    n[8] += 1;
                }
            }
            _ => {
                // time passes: hours to (rarely) months
                let dt = if rng.below(20) == 0 { rng.below(200 * DAY as u64) } else { rng.below(2 * DAY as u64) };
                env.set_time(now + dt as i64);
                // keep the epoch cursor near the clock
                let cur = ((env.now() - T0) / DAY) as u64;
                if cur > next_epoch + 1 {
                    next_epoch = cur.saturating_sub(1);
                }
            }
        }
    }
    env.drain();
    assert_eq!(env.avail(), MINING + env.m.recycled - env.chain().awarded);
    eprintln!("seed {seed:#x}: closes/publishes/withdraws/republishes/finalizes/expires/claims/releases/bounties/rejected = {n:?}");
    assert!(n.iter().all(|&k| k >= 3), "every transition kind must be exercised: {n:?}");
}

#[test]
fn property_random_transitions_seed_1() {
    random_run(0x9E37_79B9_7F4A_7C15, 1_500);
}

#[test]
fn property_random_transitions_seed_2() {
    random_run(0xD1B5_4A32_D192_ED03, 1_500);
}

#[test]
fn property_random_transitions_seed_3() {
    random_run(0x2545_F491_4F6C_DD1D, 1_500);
}

// ============================================== devnet time-scale build

#[test]
fn devnet_time_scale_build_scales_every_window_and_vesting() {
    // Only the devnet build accepts a 24 s epoch; production rejects it (above).
    let mut env = genesis_with(true, 24);
    assert_eq!(env.chain().epoch_seconds, 24);
    // close window [22 s, 30 s)
    env.try_create(0).unwrap();
    env.set_time(T0 + 21);
    expect_err(env.try_close(0), OUTSIDE_CLOSE_WINDOW);
    env.set_time(T0 + 30);
    expect_err(env.try_close(0), OUTSIDE_CLOSE_WINDOW);
    env.set_time(T0 + 29);
    let k = env.try_close(0).unwrap();
    assert_eq!(k, G2[0]);
    assert_eq!(env.m.epochs[&0].deadline, T0 + 29 + 48);
    env.try_publish(0, k, false).unwrap();
    assert_eq!(env.m.epochs[&0].open_ts, T0 + 29 + 72);
    // vesting: cliff = 365 epochs, end = 5 * 365 epochs
    env.set_time(T0 + 365 * 24 - 1);
    expect_err(env.try_release(), NOTHING_VESTED);
    env.set_time(T0 + 365 * 24 + 1);
    assert_eq!(env.try_release().unwrap(), FOUNDER / (4 * 365 * 24));
    env.set_time(T0 + 5 * 365 * 24);
    env.try_release().unwrap();
    assert_eq!(env.chain().founder_claimed, FOUNDER);
    // bad epoch lengths are still rejected by the devnet build
    let (mut svm, program, admin) = base(true);
    let m = create_mint(&mut svm, &admin, &admin.pubkey(), None, 6);
    for bad in [0u64, 11, 25, 86_401] {
        expect_err(send(&mut svm, &[ix_initialize(&program, &admin.pubkey(), &m, &admin.pubkey(), bad)], &admin, &[]), BAD_EPOCH_SECONDS);
    }
}
