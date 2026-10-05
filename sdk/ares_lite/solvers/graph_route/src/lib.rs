//! ARES Lite starter solver for CODE-SYNTH v0 GRAPH-ROUTE.
//!
//! The module must satisfy the frozen ARES-WASM-V0 profile
//! (docs/P0_T02_CODE_SYNTH_CHALLENGE_v0.md §7): integer-only, no data
//! section, imported `ares.memory`, exported `solve(i32,i32,i32,i32)->i32`.
//! Input is canonical CBOR `[vertex_count, edges, source, target]`; output is
//! canonical CBOR `[min_cost, edge_indices]` where the answer is the
//! lexicographically smallest `(cost, path)` over walks of 1..=HOP_LIMIT
//! edges that avoid the forbidden class, stay within the resource budget and
//! use the required class (see `ares_protocol.code_synth.evaluators`).
//!
//! Season-specific constants live in `season.rs`, generated from the season
//! manifest by `lite/cli/ares_lite.py starter-kit`.
#![no_std]
#![allow(unused_unsafe)]

mod season;

pub use season::{FORBIDDEN_MASK, HOP_LIMIT, REQUIRED_MASK, RESOURCE_BUDGET};

// Fixed workspace sizes. Default: sized for the frozen CODE-SYNTH v0 profiles
// (vertex_max <= 22, edge_max <= 56). `wide`: oversized (calibration only).
// `scaled`: the ARES Lite scaled GRAPH-ROUTE configuration (Lite profile L1).
#[cfg(feature = "scaled")]
const MAX_EDGES: usize = 512;
#[cfg(all(feature = "wide", not(feature = "scaled")))]
const MAX_EDGES: usize = 256;
#[cfg(not(any(feature = "wide", feature = "scaled")))]
const MAX_EDGES: usize = 64;

// ARES-WASM-V0 forbids data sections. Any reachable Rust panic embeds its
// source location as static data, so array accesses below go through these
// unchecked helpers; every index is bounded explicitly before use.
macro_rules! get {
    ($array:expr, $index:expr) => {
        *unsafe { $array.get_unchecked($index) }
    };
}
macro_rules! set {
    ($array:expr, $index:expr, $value:expr) => {
        *unsafe { $array.get_unchecked_mut($index) } = $value
    };
}
#[cfg(any(feature = "wide", feature = "scaled"))]
const MAX_VERTICES: usize = 64;
#[cfg(not(any(feature = "wide", feature = "scaled")))]
const MAX_VERTICES: usize = 24;
#[cfg(any(feature = "wide", feature = "scaled"))]
const MAX_HOPS: usize = 16;
#[cfg(not(any(feature = "wide", feature = "scaled")))]
const MAX_HOPS: usize = 8;

// Scratch tables for `memo` / `bound` live in linear memory above the host
// input/output region. The host instantiates fresh, zeroed memory for every
// test, so "0" doubles as "empty" and no initialisation fuel is spent.
const WORK_BASE: usize = 128 * 1024;
// End of usable scratch memory: memory size minus the 32 KiB shadow stack.
#[cfg(feature = "scaled")]
const WORK_END: usize = 16 * 65536 - 32 * 1024;
#[cfg(not(feature = "scaled"))]
const WORK_END: usize = 8 * 65536 - 32 * 1024;

#[cfg(target_arch = "wasm32")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn head(&mut self, major: u8) -> Option<u64> {
        let first = *self.bytes.get(self.at)?;
        self.at += 1;
        if first >> 5 != major {
            return None;
        }
        let info = first & 31;
        if info < 24 {
            return Some(info as u64);
        }
        if info > 27 {
            return None;
        }
        // 1, 2, 4 or 8 bytes; computed rather than tabled (no data section).
        let width = 1u32 << (info - 24);
        let mut value = 0u64;
        for _ in 0..width {
            value = (value << 8) | *self.bytes.get(self.at)? as u64;
            self.at += 1;
        }
        Some(value)
    }
}

struct Writer<'a> {
    bytes: &'a mut [u8],
    at: usize,
}

impl<'a> Writer<'a> {
    fn head(&mut self, major: u8, value: u64) -> Option<()> {
        let (info, width) = if value < 24 {
            (value as u8, 0)
        } else if value <= 0xff {
            (24, 1)
        } else if value <= 0xffff {
            (25, 2)
        } else if value <= 0xffff_ffff {
            (26, 4)
        } else {
            (27, 8)
        };
        self.push((major << 5) | info)?;
        let mut shift = width * 8;
        while shift > 0 {
            shift -= 8;
            self.push((value >> shift) as u8)?;
        }
        Some(())
    }

    fn push(&mut self, byte: u8) -> Option<()> {
        *self.bytes.get_mut(self.at)? = byte;
        self.at += 1;
        Some(())
    }
}

/// Parsed graph. Edges are canonically sorted by (from, to, ...), so the
/// outgoing edges of each vertex form one contiguous ascending index range.
// Per-edge storage width. `compact` relies on the frozen GRAPH-ROUTE bounds
// (cost <= cost_max <= 48, per-edge resource <= resource_budget, class < 8)
// and rejects any input that would not fit, so it can never answer wrongly.
#[cfg(not(feature = "compact"))]
type Field = u64;
#[cfg(feature = "compact")]
type Field = u16;

pub struct Graph {
    vertex_count: usize,
    edge_count: usize,
    to: [u8; MAX_EDGES],
    cost: [Field; MAX_EDGES],
    resource: [Field; MAX_EDGES],
    class_bit: [Field; MAX_EDGES],
    first: [u16; MAX_VERTICES + 1],
    source: usize,
    target: usize,
}

pub fn parse(input: &[u8]) -> Option<Graph> {
    let mut reader = Reader { bytes: input, at: 0 };
    if reader.head(4)? != 4 {
        return None;
    }
    let vertex_count = reader.head(0)? as usize;
    let edge_count = reader.head(4)? as usize;
    if vertex_count > MAX_VERTICES || edge_count > MAX_EDGES {
        return None;
    }
    let mut graph = Graph {
        vertex_count,
        edge_count,
        to: [0; MAX_EDGES],
        cost: [0; MAX_EDGES],
        resource: [0; MAX_EDGES],
        class_bit: [0; MAX_EDGES],
        first: [0; MAX_VERTICES + 1],
        source: 0,
        target: 0,
    };
    let mut counts = [0u16; MAX_VERTICES];
    for index in 0..edge_count {
        if reader.head(4)? != 5 {
            return None;
        }
        let from = reader.head(0)? as usize;
        let to = reader.head(0)? as usize;
        if from >= vertex_count || to >= vertex_count {
            return None;
        }
        set!(graph.to, index, to as u8);
        let cost = reader.head(0)?;
        let resource = reader.head(0)?;
        let class_id = reader.head(0)?;
        if cost > Field::MAX as u64 || resource > Field::MAX as u64 || class_id >= Field::BITS as u64 {
            return None;
        }
        set!(graph.cost, index, cost as Field);
        set!(graph.resource, index, resource as Field);
        set!(graph.class_bit, index, (1 as Field) << class_id);
        set!(counts, from, get!(counts, from) + 1);
    }
    graph.source = reader.head(0)? as usize;
    graph.target = reader.head(0)? as usize;
    if graph.source >= vertex_count || graph.target >= vertex_count {
        return None;
    }
    let mut running = 0u16;
    let mut vertex = 0;
    while vertex <= MAX_VERTICES {
        set!(graph.first, vertex, running);
        if vertex < vertex_count {
            running += get!(counts, vertex);
        }
        vertex += 1;
    }
    Some(graph)
}

pub struct Answer {
    pub cost: u64,
    pub length: usize,
    pub path: [u16; MAX_HOPS],
}

/// Depth-first enumeration of walks in ascending edge order. Pre-order DFS
/// visits walks in lexicographic order, so the first walk found at a given
/// cost is the lexicographically smallest one with that cost.
pub fn search(graph: &Graph) -> Option<Answer> {
    let hops = (HOP_LIMIT as usize).min(MAX_HOPS);
    let mut best = Answer { cost: u64::MAX, length: 0, path: [0; MAX_HOPS] };
    let mut found = false;
    let mut path = [0u16; MAX_HOPS];
    let mut next = [0u16; MAX_HOPS + 1];
    let mut end = [0u16; MAX_HOPS + 1];
    let mut cost = [0u64; MAX_HOPS + 1];
    let mut used = [0u64; MAX_HOPS + 1];
    let mut seen = [0u64; MAX_HOPS + 1];
    // lower[h * MAX_VERTICES + v]: cheapest cost from v to the target using at
    // most h more edges, ignoring resource and class constraints.
    let mut lower = [INFINITE; (MAX_HOPS + 1) * MAX_VERTICES];
    if cfg!(feature = "lb") {
        lower_bounds(graph, hops, &mut lower);
    }
    if cfg!(feature = "bound") {
        admissible_bounds(graph, hops);
    }
    // The memo table must fit in scratch memory; otherwise run without it
    // (still exact, just slower).
    let memo_entries = (hops + 1) * graph.vertex_count * (RESOURCE_BUDGET as usize + 1) * 2;
    let use_memo = cfg!(feature = "memo") && WORK_BASE + 8 * (MEMO + memo_entries) <= WORK_END;
    next[0] = get!(graph.first, graph.source);
    end[0] = get!(graph.first, graph.source + 1);
    let mut depth = 0usize;
    loop {
        if get!(next, depth) >= get!(end, depth) {
            if depth == 0 {
                break;
            }
            depth -= 1;
            continue;
        }
        let edge = get!(next, depth) as usize;
        set!(next, depth, edge as u16 + 1);
        if edge >= graph.edge_count {
            return None;
        }
        let class_bit = get!(graph.class_bit, edge) as u64;
        if class_bit & FORBIDDEN_MASK != 0 {
            continue;
        }
        let resource = get!(used, depth) + get!(graph.resource, edge) as u64;
        if resource > RESOURCE_BUDGET {
            continue;
        }
        let total = get!(cost, depth) + get!(graph.cost, edge) as u64;
        if !cfg!(feature = "naive") && found && total >= best.cost {
            continue;
        }
        let vertex = get!(graph.to, edge) as usize;
        let remaining = hops - depth - 1;
        if cfg!(feature = "lb") && vertex != graph.target {
            let bound = get!(lower, remaining * MAX_VERTICES + vertex);
            if bound == INFINITE || (found && total + bound >= best.cost) {
                continue;
            }
        }
        let mask = get!(seen, depth) | (class_bit & REQUIRED_MASK);
        let have = (mask == REQUIRED_MASK) as usize;
        if cfg!(feature = "bound") {
            // Admissible bounds: cheapest cost / least resource to finish
            // from (vertex, remaining hops, required class seen?).
            let at = bound_index(graph, remaining, vertex, have);
            let cost_left = table_get(BOUND_COST, at);
            if cost_left == 0 || (found && total + cost_left - 1 >= best.cost) {
                continue;
            }
            if resource + table_get(BOUND_RESOURCE, at) - 1 > RESOURCE_BUDGET {
                continue;
            }
        }
        if use_memo {
            // Dominance: an earlier visit of the same (depth, vertex,
            // resource, class) state with cost <= total came first in
            // lexicographic order, so every completion of this walk is
            // dominated by the same completion of that one.
            let at = memo_index(graph, depth + 1, vertex, resource as usize, have);
            let seen_cost = table_get(MEMO, at);
            if seen_cost != 0 && seen_cost - 1 <= total {
                continue;
            }
            table_set(MEMO, at, total + 1);
        }
        set!(path, depth, edge as u16);
        if vertex == graph.target && mask == REQUIRED_MASK && (!found || total < best.cost) {
            found = true;
            best.cost = total;
            best.length = depth + 1;
            let mut index = 0;
            while index <= depth {
                set!(best.path, index, get!(path, index));
                index += 1;
            }
        }
        if depth + 1 < hops {
            depth += 1;
            set!(cost, depth, total);
            set!(used, depth, resource);
            set!(seen, depth, mask);
            set!(next, depth, get!(graph.first, vertex));
            set!(end, depth, get!(graph.first, vertex + 1));
        }
    }
    if found {
        Some(best)
    } else {
        None
    }
}

const INFINITE: u64 = u64::MAX;

// Table regions (u64 slots) in scratch memory. Values are stored as v + 1,
// 0 meaning "unknown/infinite".
const BOUND_COST: usize = 0;
const BOUND_RESOURCE: usize = (MAX_HOPS + 1) * MAX_VERTICES * 2;
const MEMO: usize = 2 * (MAX_HOPS + 1) * MAX_VERTICES * 2;

fn table_get(region: usize, index: usize) -> u64 {
    unsafe { *((WORK_BASE + 8 * (region + index)) as *const u64) }
}

fn table_set(region: usize, index: usize, value: u64) {
    unsafe { *((WORK_BASE + 8 * (region + index)) as *mut u64) = value }
}

fn bound_index(_graph: &Graph, hops_left: usize, vertex: usize, have: usize) -> usize {
    (hops_left * MAX_VERTICES + vertex) * 2 + have
}

fn memo_index(graph: &Graph, depth: usize, vertex: usize, resource: usize, have: usize) -> usize {
    (((depth * graph.vertex_count + vertex) * (RESOURCE_BUDGET as usize + 1)) + resource) * 2 + have
}

/// Reverse Bellman-Ford over (hops left, vertex, required-class seen):
/// minimum cost and minimum resource to reach the target legally.
fn admissible_bounds(graph: &Graph, hops: usize) {
    table_set(BOUND_COST, bound_index(graph, 0, graph.target, 1), 1);
    table_set(BOUND_RESOURCE, bound_index(graph, 0, graph.target, 1), 1);
    let mut h = 1;
    while h <= hops {
        let mut vertex = 0;
        while vertex < graph.vertex_count {
            let mut have = 0;
            while have < 2 {
                let mut best_cost = table_get(BOUND_COST, bound_index(graph, h - 1, vertex, have));
                let mut best_resource = table_get(BOUND_RESOURCE, bound_index(graph, h - 1, vertex, have));
                let mut edge = get!(graph.first, vertex) as usize;
                let end = get!(graph.first, vertex + 1) as usize;
                while edge < end && edge < graph.edge_count {
                    let class_bit = get!(graph.class_bit, edge) as u64;
                    if class_bit & FORBIDDEN_MASK == 0 {
                        let next_have = have | ((class_bit & REQUIRED_MASK != 0) as usize);
                        let at = bound_index(graph, h - 1, get!(graph.to, edge) as usize, next_have);
                        let cost = table_get(BOUND_COST, at);
                        if cost != 0 {
                            let candidate = cost + get!(graph.cost, edge) as u64;
                            if best_cost == 0 || candidate < best_cost {
                                best_cost = candidate;
                            }
                        }
                        let resource = table_get(BOUND_RESOURCE, at);
                        if resource != 0 {
                            let candidate = resource + get!(graph.resource, edge) as u64;
                            if best_resource == 0 || candidate < best_resource {
                                best_resource = candidate;
                            }
                        }
                    }
                    edge += 1;
                }
                table_set(BOUND_COST, bound_index(graph, h, vertex, have), best_cost);
                table_set(BOUND_RESOURCE, bound_index(graph, h, vertex, have), best_resource);
                have += 1;
            }
            vertex += 1;
        }
        h += 1;
    }
}

fn lower_bounds(graph: &Graph, hops: usize, lower: &mut [u64; (MAX_HOPS + 1) * MAX_VERTICES]) {
    set!(lower, graph.target, 0);
    let mut h = 1;
    while h <= hops {
        let mut vertex = 0;
        while vertex < graph.vertex_count {
            let mut best = get!(lower, (h - 1) * MAX_VERTICES + vertex);
            let mut edge = get!(graph.first, vertex) as usize;
            let end = get!(graph.first, vertex + 1) as usize;
            while edge < end && edge < graph.edge_count {
                if get!(graph.class_bit, edge) as u64 & FORBIDDEN_MASK == 0 {
                    let tail = get!(lower, (h - 1) * MAX_VERTICES + get!(graph.to, edge) as usize);
                    let cost = get!(graph.cost, edge) as u64;
                    if tail != INFINITE && cost + tail < best {
                        best = cost + tail;
                    }
                }
                edge += 1;
            }
            set!(lower, h * MAX_VERTICES + vertex, best);
            vertex += 1;
        }
        h += 1;
    }
}

pub fn encode(answer: &Answer, output: &mut [u8]) -> Option<usize> {
    let mut writer = Writer { bytes: output, at: 0 };
    writer.head(4, 2)?;
    writer.head(0, answer.cost)?;
    writer.head(4, answer.length as u64)?;
    let mut index = 0;
    while index < answer.length && index < MAX_HOPS {
        writer.head(0, get!(answer.path, index) as u64)?;
        index += 1;
    }
    Some(writer.at)
}

pub fn solve_bytes(input: &[u8], output: &mut [u8]) -> i32 {
    let written = parse(input)
        .and_then(|graph| search(&graph))
        .and_then(|answer| encode(&answer, output));
    match written {
        Some(length) => length as i32,
        None => -1,
    }
}

/// ARES-WASM-V0 entry point.
///
/// # Safety
/// The host guarantees `[input, input+len)` and `[output, output+cap)` are
/// disjoint, in-bounds regions of the module's single linear memory.
#[no_mangle]
pub unsafe extern "C" fn solve(input: i32, len: i32, output: i32, cap: i32) -> i32 {
    let input = core::slice::from_raw_parts(input as usize as *const u8, len as usize);
    let output = core::slice::from_raw_parts_mut(output as usize as *mut u8, cap as usize);
    solve_bytes(input, output)
}
