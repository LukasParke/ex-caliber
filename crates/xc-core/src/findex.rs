//! Fractional ordering keys — faithful Rust port of rocicorp/fractional-indexing
//! (default alphabets only, which is exactly what Excalidraw serializes).
//!
//! Reference: https://github.com/rocicorp/fractional-indexing (CC0), based on
//! https://observablehq.com/@dgreensp/implementing-fractional-indexing
//!
//! Keys sort lexicographically as UTF-8 byte strings; `Vec<String>` order equals
//! z-order. No trailing `'0'` digits are ever produced.

const DIGITS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
/// Head alphabet marking integer-part magnitude/length: A-Z negative side, a-z positive.
const INT_DIGITS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

const ZERO: u8 = b'0';
/// Smallest representable integer part: `"A"` followed by 26 zeros.
const SMALLEST_INTEGER: &str = "A00000000000000000000000000";
const HALF: usize = INT_DIGITS.len() / 2;

fn digit_index(c: u8) -> u32 {
    match c {
        b'0'..=b'9' => (c - b'0') as u32,
        b'A'..=b'Z' => (c - b'A') as u32 + 10,
        b'a'..=b'z' => (c - b'a') as u32 + 36,
        _ => panic!("invalid fractional-index digit: {}", c as char),
    }
}

fn int_head_index(c: u8) -> u32 {
    INT_DIGITS
        .iter()
        .position(|&d| d == c)
        .unwrap_or_else(|| panic!("invalid order key head: {}", c as char)) as u32
}

fn integer_length(head: u8) -> usize {
    let i = int_head_index(head) as usize;
    if i < HALF { HALF - i + 1 } else { i - HALF + 2 }
}

fn get_integer_part(key: &str) -> Result<&str, String> {
    let len = integer_length(
        *key.as_bytes()
            .first()
            .ok_or_else(|| "empty order key".to_string())?,
    );
    if len > key.len() {
        return Err(format!("invalid order key: {key}"));
    }
    Ok(&key[..len])
}

fn validate_order_key(key: &str) -> Result<(), String> {
    if key == SMALLEST_INTEGER {
        return Err(format!("invalid order key: {key}"));
    }
    let ip = get_integer_part(key)?;
    if key[ip.len()..].ends_with(ZERO as char) {
        return Err(format!("invalid order key: {key}"));
    }
    Ok(())
}

fn is_smallest_integer(x: &str) -> bool {
    x == SMALLEST_INTEGER
}

/// `a` may be empty; `b` is `None` or non-empty; `a < b` lexicographically when `b` present.
fn midpoint(a: &str, b: Option<&str>) -> Result<String, String> {
    if let Some(b) = b
        && a >= b
    {
        return Err(format!("{a} >= {b}"));
    }
    if a.ends_with(ZERO as char) || b.is_some_and(|b| b.ends_with(ZERO as char)) {
        return Err("trailing zero".to_string());
    }

    if let Some(b) = b {
        let (ab, bb) = (a.as_bytes(), b.as_bytes());
        let mut n = 0;
        while n < bb.len() && ab.get(n).copied().unwrap_or(ZERO) == bb[n] {
            n += 1;
        }
        if n > 0 {
            return Ok(b[..n].to_string() + &midpoint(&a[n.min(ab.len())..], Some(&b[n..]))?);
        }
    }

    let digit_a = a.as_bytes().first().map(|&c| digit_index(c)).unwrap_or(0);
    let digit_b = match b {
        Some(b) => digit_index(b.as_bytes()[0]),
        None => DIGITS.len() as u32,
    };
    if digit_b - digit_a > 1 {
        let mid_digit = (digit_a + digit_b).div_ceil(2) as usize; // round((a+b)/2)
        Ok((DIGITS[mid_digit] as char).to_string())
    } else if b.is_some_and(|b| b.len() > 1) {
        Ok((b.unwrap().as_bytes()[0] as char).to_string())
    } else {
        // First digits consecutive and `b` absent or single-digit: recurse deeper.
        let mut out = (DIGITS[digit_a as usize] as char).to_string();
        out.push_str(&midpoint(&a[1.min(a.len())..], None)?);
        Ok(out)
    }
}

fn increment_integer(x: &str) -> Option<String> {
    let xb = x.as_bytes();
    let head = xb[0];
    let mut trailing = String::new();
    for i in (1..xb.len()).rev() {
        let d = digit_index(xb[i]) + 1;
        if d == DIGITS.len() as u32 {
            trailing.insert(0, ZERO as char);
        } else {
            return Some(
                (head as char).to_string()
                    + std::str::from_utf8(&xb[1..i]).unwrap()
                    + &(DIGITS[d as usize] as char).to_string()
                    + &trailing,
            );
        }
    }
    let head_index = int_head_index(head) as usize;
    if head_index == INT_DIGITS.len() - 1 {
        return None; // largest integer
    }
    let h = INT_DIGITS[head_index + 1];
    let length_delta = integer_length(h) as isize - integer_length(head) as isize;
    let tail = if length_delta > 0 {
        format!("{trailing}{}", ZERO as char)
    } else if length_delta < 0 {
        trailing[1..].to_string()
    } else {
        trailing
    };
    Some(format!("{}{}", h as char, tail))
}

fn decrement_integer(x: &str) -> Option<String> {
    let xb = x.as_bytes();
    let head = xb[0];
    let last = DIGITS[DIGITS.len() - 1];
    let mut trailing = String::new();
    for i in (1..xb.len()).rev() {
        let di = digit_index(xb[i]);
        if di == 0 {
            trailing.insert(0, last as char);
        } else {
            return Some(
                (head as char).to_string()
                    + std::str::from_utf8(&xb[1..i]).unwrap()
                    + &(DIGITS[(di - 1) as usize] as char).to_string()
                    + &trailing,
            );
        }
    }
    let head_index = int_head_index(head) as usize;
    if head_index == 0 {
        return None; // smallest integer
    }
    let h = INT_DIGITS[head_index - 1];
    let length_delta = integer_length(h) as isize - integer_length(head) as isize;
    let tail = if length_delta > 0 {
        format!("{trailing}{}", last as char)
    } else if length_delta < 0 {
        trailing[1..].to_string()
    } else {
        trailing
    };
    Some(format!("{}{}", h as char, tail))
}

/// Key strictly between `a` and `b`; either may be `None` (open end).
///
/// Errors mirror the reference implementation's throw sites: malformed input keys
/// (which can come from untrusted files) and exhausted integer space.
pub fn generate_key_between(a: Option<&str>, b: Option<&str>) -> Result<String, String> {
    if let Some(k) = a {
        validate_order_key(k)?;
    }
    if let Some(k) = b {
        validate_order_key(k)?;
    }
    let (mut a, mut b) = (a, b);
    if let (Some(av), Some(bv)) = (a, b)
        && av > bv
    {
        std::mem::swap(&mut a, &mut b);
    }

    if a.is_none() && b.is_none() {
        let head = INT_DIGITS[HALF];
        return Ok(format!("{}{}", head as char, ZERO as char));
    }

    if let Some(bv) = b.filter(|_| a.is_none()) {
        let ib = get_integer_part(bv)?;
        let fb = &bv[ib.len()..];
        if is_smallest_integer(ib) {
            return midpoint("", Some(fb)).map(|m| format!("{}{}", ib, m));
        }
        if ib < bv {
            return Ok(ib.to_string());
        }
        return decrement_integer(ib).ok_or_else(|| "cannot decrement any more".to_string());
    }

    if let Some(av) = a.filter(|_| b.is_none()) {
        let ia = get_integer_part(av)?;
        let fa = &av[ia.len()..];
        return match increment_integer(ia) {
            None => midpoint(fa, None).map(|m| format!("{}{}", ia, m)),
            Some(i) => Ok(i),
        };
    }

    // Both bounds present (post-swap guarantees a < b).
    let (av, bv) = (a.unwrap(), b.unwrap());
    let ia = get_integer_part(av)?;
    let fa = &av[ia.len()..];
    let ib = get_integer_part(bv)?;
    let fb = &bv[ib.len()..];

    if ia == ib {
        return midpoint(fa, Some(fb)).map(|m| format!("{}{}", ia, m));
    }
    let i = increment_integer(ia).ok_or_else(|| "cannot increment any more".to_string())?;
    if i.as_str() < bv {
        return Ok(i);
    }
    midpoint(fa, None).map(|m| format!("{}{}", ia, m))
}

/// `n` distinct keys in sorted order between `a` and `b` (either open-ended).
pub fn generate_n_keys_between(a: Option<&str>, b: Option<&str>, n: usize) -> Vec<String> {
    match n {
        0 => vec![],
        1 => vec![generate_key_between(a, b).expect("valid key request")],
        _ => {
            if b.is_none() {
                let mut c = generate_key_between(a, None).expect("valid key request");
                let mut result = vec![c.clone()];
                for _ in 0..n - 1 {
                    c = generate_key_between(Some(&c), None).expect("valid key request");
                    result.push(c.clone());
                }
                result
            } else if a.is_none() {
                let mut c = generate_key_between(None, b).expect("valid key request");
                let mut result = vec![c.clone()];
                for _ in 0..n - 1 {
                    c = generate_key_between(None, Some(&c)).expect("valid key request");
                    result.push(c.clone());
                }
                result.reverse();
                result
            } else {
                let mid = n / 2;
                let c = generate_key_between(a, b).expect("valid key request");
                let mut out = generate_n_keys_between(a, Some(&c), mid);
                out.push(c.clone());
                out.extend(generate_n_keys_between(Some(&c), b, n - mid - 1));
                out
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vectors produced by running the reference implementation (node, CC0 src).
    #[test]
    fn matches_reference_vectors() {
        let cases: Vec<(Option<&str>, Option<&str>, &str)> = vec![
            (None, None, "a0"),
            (None, Some("a0"), "Zz"),
            (Some("a0"), None, "a1"),
            (Some("a0"), Some("a1"), "a0V"),
            (Some("a1"), Some("a2"), "a1V"),
            (Some("a0V"), Some("a1"), "a0l"),
            (None, Some("Zz"), "Zy"),
            (Some("aT"), Some("aU"), "aTV"),
            (Some("Zz"), Some("a0"), "ZzV"),
        ];
        for (a, b, want) in cases {
            assert_eq!(generate_key_between(a, b).unwrap(), want, "a={a:?} b={b:?}");
        }
    }

    #[test]
    fn rejects_reference_error_cases() {
        for bad in ["b0", "a00", "Azzzz"] {
            assert!(generate_key_between(Some(bad), None).is_err(), "{bad}");
        }
        assert!(generate_key_between(Some("aZ"), Some("b0")).is_err());
    }

    #[test]
    fn append_prepend_sequences() {
        // Append five: a0 a1 a2 a3 a4
        let mut k: Option<String> = None;
        let mut seq = vec![];
        for _ in 0..5 {
            k = Some(generate_key_between(k.as_deref(), None).unwrap());
            seq.push(k.clone().unwrap());
        }
        assert_eq!(seq, ["a0", "a1", "a2", "a3", "a4"]);

        // Prepend three before a0: Zx Zy Zz
        let mut k = "a0".to_string();
        let mut pre = vec![];
        for _ in 0..3 {
            k = generate_key_between(None, Some(&k)).unwrap();
            pre.push(k.clone());
        }
        assert_eq!(pre, ["Zz", "Zy", "Zx"]);
    }

    #[test]
    fn n_keys_are_sorted_and_between() {
        let keys = generate_n_keys_between(Some("a0"), Some("a1"), 3);
        assert_eq!(keys, ["a0G", "a0V", "a0l"]);
        let keys = generate_n_keys_between(Some("a9"), None, 3);
        assert_eq!(keys, ["aA", "aB", "aC"]);
    }

    /// Deterministic stress: interleaved insertions anywhere must preserve strict
    /// lexicographic order, and keys must stay short (no runaway fraction growth).
    #[test]
    fn random_interleaved_insertions_stay_ordered() {
        let lcg = |state: &mut u64| {
            *state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (*state >> 33) as usize
        };
        let mut state = 42u64;
        let mut keys: Vec<String> = generate_n_keys_between(None, None, 10);
        for _ in 0..2000 {
            let pos = lcg(&mut state) % (keys.len() + 1);
            let k = if pos == 0 {
                generate_key_between(None, keys.first().map(|s| s.as_str())).unwrap()
            } else if pos == keys.len() {
                generate_key_between(keys.last().map(|s| s.as_str()), None).unwrap()
            } else {
                generate_key_between(Some(keys[pos - 1].as_str()), Some(keys[pos].as_str()))
                    .unwrap()
            };
            keys.insert(pos, k);
            assert!(keys.windows(2).all(|w| w[0] < w[1]), "order broken");
        }
        assert!(keys.iter().all(|k| k.len() <= 40), "key growth runaway");
    }
}
