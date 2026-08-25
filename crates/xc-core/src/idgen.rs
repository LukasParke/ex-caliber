//! Id / seed / timestamp generation. Excalidraw ids are random base-36-ish strings;
//! seeds are arbitrary nonzero integers. A time-seeded LCG is sufficient (ids only
//! need uniqueness within a document's lifetime, not cryptographic strength).

use std::sync::atomic::{AtomicU64, Ordering};

static STATE: AtomicU64 = AtomicU64::new(0);

fn next_u64() -> u64 {
    let s = STATE.fetch_add(1, Ordering::Relaxed);
    let seed = if s == 0 { init_seed() } else { s };

    seed.wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407 ^ std::process::id() as u64)
}

fn init_seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E3779B97F4A7C15)
}

const ID_ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// New element id, Excalidraw-length (20 chars).
pub fn new_id() -> String {
    let mut id = String::with_capacity(20);
    for i in 0..20 {
        let r = next_u64();
        id.push(ID_ALPHABET[(r >> (i % 10)) as usize % ID_ALPHABET.len()] as char);
    }
    id
}

/// New rough-geometry seed; preserves a provided nonzero seed.
pub fn new_seed(existing: Option<i64>) -> i64 {
    match existing {
        Some(s) if s != 0 => s,
        // Take the top 63 bits' worth of entropy as positive i64; a `.min()`
        // clamp here collapsed every large draw to i64::MAX (identical seeds).
        _ => ((next_u64() >> 1) as i64) | 1,
    }
}
