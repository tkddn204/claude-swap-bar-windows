//! Test double for `cswap.exe`. Behavior is selected by `FAKE_CSWAP_MODE`.

use std::io::Write;

fn main() {
    let mode = std::env::var("FAKE_CSWAP_MODE").unwrap_or_else(|_| "ok".to_string());
    match mode.as_str() {
        "ok" => {
            print!(
                "{}",
                include_str!("../tests/fixtures/list_two_accounts.json")
            );
        }
        "fail" => {
            let mut err = std::io::stderr();
            writeln!(err, "cswap: no accounts configured").unwrap();
            writeln!(err, "second line of noise").unwrap();
            std::process::exit(3);
        }
        "cp949-stderr" => {
            // "계정 없음" encoded as cp949 — what a Python child actually
            // writes to a redirected pipe on a Korean Windows install. These
            // bytes are not valid UTF-8.
            let bytes: &[u8] = &[0xB0, 0xE8, 0xC1, 0xA4, 0x20, 0xBE, 0xF8, 0xC0, 0xBD, 0x0A];
            std::io::stderr().write_all(bytes).unwrap();
            std::process::exit(4);
        }
        "hang" => {
            // Must outlive run_owned's 10s deadline, with margin, so the
            // timeout test observes a kill rather than a natural exit.
            std::thread::sleep(std::time::Duration::from_secs(30));
        }
        "linger" => {
            // Long enough that a fire-and-forget spawn is still running when
            // the assertion checks, short enough not to hold a lock on this
            // executable into the next build.
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
        "badjson" => {
            print!("{{ not json");
        }
        "echo-args" => {
            let args: Vec<String> = std::env::args().skip(1).collect();
            print!("{}", args.join(" "));
        }
        other => {
            eprintln!("unknown FAKE_CSWAP_MODE: {other}");
            std::process::exit(1);
        }
    }
}
