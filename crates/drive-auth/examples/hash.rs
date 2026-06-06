//! Helper: print an Argon2id PHC string for a password passed on the CLI.
//! Used by deployment scripts and tests to populate `DRIVE_ADMIN_PASSWORD_HASH`.
//!
//! Usage:
//!   cargo run --quiet --example hash -p drive-auth -- <password>

fn main() {
    let pw = std::env::args().nth(1).expect("usage: hash <password>");
    let h = drive_auth::hash_password(&pw).expect("hash_password");
    println!("{h}");
}
