//! Run installed-font integration independently of portable file-backed discovery tests.
use std::process::Command;

const TEST: &str = "fonts::tests::native_system_font_scan";

fn validate_listing(stdout: &[u8]) -> Result<(), String> {
    let text = std::str::from_utf8(stdout).map_err(|e| format!("invalid native-font test listing: {e}"))?;
    let expected = format!("{TEST}: test");
    let count = text.lines().filter(|line| *line == expected).count();
    if count == 1 { Ok(()) } else { Err(format!("expected exactly one {TEST} test, found {count}")) }
}

pub fn run() -> Result<(), String> {
    if !cfg!(any(target_os = "macos", target_os = "linux")) {
        return Err("native-fonts requires a macOS or Linux host".into());
    }
    let args = ["test", "--release", "-p", "effectcraft-text", "--lib", TEST, "--", "--ignored", "--exact"];
    let listing =
        Command::new(env!("CARGO")).args(args).arg("--list").current_dir(crate::root()).output().map_err(|e| format!("cannot list native-font test: {e}"))?;
    if !listing.status.success() {
        return Err(format!("listing native-font test failed: {}", String::from_utf8_lossy(&listing.stderr)));
    }
    validate_listing(&listing.stdout)?;
    crate::run(Command::new(env!("CARGO")).args(args).arg("--nocapture").current_dir(crate::root()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn listing_must_contain_one_exact_native_test() {
        assert!(validate_listing(format!("{TEST}: test\n1 test, 0 benchmarks\n").as_bytes()).is_ok());
        for listing in [String::new(), "0 tests, 0 benchmarks\n".into(), "other::test: test\n".into(), format!("{TEST}: test\n{TEST}: test\n")] {
            assert!(validate_listing(listing.as_bytes()).is_err(), "{listing:?}");
        }
    }
}
