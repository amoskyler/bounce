//! Open a database through [`Store`], creating or migrating it, and report.
//!
//! Used to check a migration against a real database rather than a synthetic
//! one:
//!
//! ```text
//! cargo run -p libbounce --example opendb -- /path/to/bounce.db
//! cargo run -p libbounce --example opendb -- /path/to/new.db --fresh
//! ```
//!
//! `--fresh` deletes the file first. It is opt-in because the whole point of
//! pointing this at a real database is that the data survives.

use libbounce::store::{schema, Store};

fn main() {
    let mut arguments = std::env::args().skip(1);
    let path = arguments.next().expect("usage: opendb <path> [--fresh]");
    let fresh = arguments.any(|argument| argument == "--fresh");

    if fresh {
        let _ = std::fs::remove_file(&path);
    }

    let before = rusqlite::Connection::open(&path)
        .ok()
        .and_then(|connection| schema::version(&connection).ok());

    let store = Store::open(&path).expect("opens");
    drop(store);

    let connection = rusqlite::Connection::open(&path).expect("reopens");
    println!(
        "{path}: version {:?} -> {}",
        before,
        schema::version(&connection).expect("reads the version"),
    );
}
