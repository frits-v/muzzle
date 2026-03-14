#![no_main]
use libfuzzer_sys::fuzz_target;
use muzzle::gitcheck;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Must not panic on any input
        let _ = gitcheck::check_bash_write_paths(s);
    }
});
