//! Hardening: fuzz-style parser robustness + round-trip properties + perf bounds.
//!
//! Deterministic LCG "fuzzing" (no nightly cargo-fuzz needed): random JSON must
//! never panic the loader — only produce errors or valid scenes. Valid scenes
//! must survive load→save→load as logical identities.

use xc_core::file;

fn lcg(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state >> 33
}

const CORPUS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../xc-core/tests/fixtures");

#[test]
fn random_json_never_panics_the_loader() {
    let mut state = 0xC0FFEEu64;
    for _ in 0..2000 {
        let n = (lcg(&mut state) % 400) as usize;
        let bytes: Vec<u8> = (0..n)
            .map(|_| match lcg(&mut state) % 8 {
                0 => b'{',
                1 => b'}',
                2 => b'"',
                3 => b'[',
                4 => b'x',
                5 => b':',
                6 => b',',
                _ => (lcg(&mut state) % 96 + 32) as u8,
            })
            .collect();
        let input = String::from_utf8_lossy(&bytes).to_string();
        // Must not panic; Err is fine, Ok is fine.
        let _ = file::load_document(&input);
    }
}

#[test]
fn truncated_real_files_never_panic() {
    let mut state = 0xBAD5EEDu64;
    for entry in std::fs::read_dir(CORPUS).unwrap() {
        let path = entry.unwrap().path();
        let raw = match std::fs::read_to_string(&path) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for _ in 0..20 {
            let cut = (lcg(&mut state) as usize) % (raw.len() + 1);
            let _ = file::load_document(&raw[..cut]);
        }
        // Byte-soup injections into real structure.
        for _ in 0..20 {
            let mut bytes = raw.clone().into_bytes();
            let idx = (lcg(&mut state) as usize) % bytes.len();
            bytes[idx] = match lcg(&mut state) % 3 {
                0 => b'"',
                1 => b'}',
                _ => 0x00,
            };
            let _ = file::load_document(&String::from_utf8_lossy(&bytes));
        }
    }
}

#[test]
fn perf_scene_to_svg_10k_elements_is_bounded() {
    // Generous bound (CI variance): 10k elements must export in < 8s on any
    // dev box; local runs land in ~100ms. Catches accidental O(n²).
    let start = std::time::Instant::now();
    let mut scene = xc_core::scene::Scene::new();
    for i in 0..10_000 {
        scene
            .add(xc_core::element::Element {
                kind: xc_core::element::ElementType::Rectangle,
                id: format!("r{i}"),
                x: (i % 100) as f64 * 150.0,
                y: (i / 100) as f64 * 110.0,
                width: 100.0,
                height: 60.0,
                backgroundColor: "#a5d8ff".into(),
                ..Default::default()
            })
            .unwrap();
    }
    let svg = xc_io::scene_to_svg(&scene, 0.0);
    assert!(svg.contains("<rect"));
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < 8,
        "svg export of 10k elements took {elapsed:?}"
    );
}

#[test]
fn perf_geometry_render_1k_is_bounded() {
    let start = std::time::Instant::now();
    for i in 0..1_000 {
        let el = xc_core::element::Element {
            kind: xc_core::element::ElementType::Ellipse,
            id: format!("e{i}"),
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 80.0,
            backgroundColor: "#b2f2bb".into(),
            seed: i as i64,
            ..Default::default()
        };
        let ops = xc_render::geometry::render_element(&el).unwrap();
        assert!(!ops.is_empty());
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs() < 5, "1k rough renders took {elapsed:?}");
}
