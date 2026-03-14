#![no_main]
use libfuzzer_sys::fuzz_target;
use muzzle::gitcheck;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Must not panic on any input
        let _ = gitcheck::extract_repo_from_git_op(s);
    }
});
