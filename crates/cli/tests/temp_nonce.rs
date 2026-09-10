//! Deterministic checks for repeated and reversed test-directory clocks.
#[path = "support/temp_nonce.rs"]
mod temp_nonce;

#[test]
fn equal_timestamps_are_unique_across_threads() {
    let mut handles = Vec::new();
    for _ in 0..6 {
        handles.push(std::thread::spawn(|| {
            (0..512)
                .map(|_| temp_nonce::unique_stamp(42))
                .collect::<Vec<_>>()
        }));
    }
    let stamps: Vec<_> = handles
        .into_iter()
        .flat_map(|h| h.join().unwrap())
        .collect();
    let unique: std::collections::BTreeSet<_> = stamps.iter().copied().collect();
    assert_eq!(unique.len(), stamps.len());
}

#[test]
fn repeated_and_reversed_clocks_still_advance() {
    let a = temp_nonce::unique_stamp(100);
    let b = temp_nonce::unique_stamp(100);
    let c = temp_nonce::unique_stamp(1);
    assert!(a < b && b < c);
}
