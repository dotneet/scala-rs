//! Process-local unique stamps for test output directories.
use std::sync::Mutex;

pub fn unique_stamp(wall_clock_nanos: u128) -> u128 {
    static LAST: Mutex<u128> = Mutex::new(0);
    let mut last = LAST.lock().unwrap();
    // Clocks may repeat or move backwards. Allocation order, not clock
    // resolution, separates concurrent tests using the same directory tag.
    *last = wall_clock_nanos.max(last.checked_add(1).expect("test nonce exhausted"));
    last.checked_mul(1_u128 << 32)
        .expect("test timestamp exhausted")
        | u128::from(std::process::id())
}
