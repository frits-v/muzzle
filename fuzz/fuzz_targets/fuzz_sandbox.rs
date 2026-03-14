#![no_main]
use libfuzzer_sys::fuzz_target;
use muzzle::sandbox;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Must not panic on any input (no session = simplest path)
        let _ = sandbox::check_path(s, None);
    }
});
