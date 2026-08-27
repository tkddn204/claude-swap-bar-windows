//! Test double for `cswap.exe`. Behavior is selected by `FAKE_CSWAP_MODE`.

use std::io::Write;

fn main() {
    let mode = std::env::var("FAKE_CSWAP_MODE").unwrap_or_else(|_| "ok".to_string());
    match mode.as_str() {
        "ok" => {
            print!(
                "{}",
                include_str!("../../tests/fixtures/list_two_accounts.json")
            );
        }
        "fail" => {
            let mut err = std::io::stderr();
            writeln!(err, "cswap: no accounts configured").unwrap();
            writeln!(err, "second line of noise").unwrap();
            std::process::exit(3);
        }
        "hang" => {
            std::thread::sleep(std::time::Duration::from_secs(60));
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
