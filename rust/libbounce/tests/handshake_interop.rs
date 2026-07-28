//! The transport handshake, checked against the Go client's own code.
//!
//! The handshake is the one part of the protocol with no version, no
//! negotiation and no error message: it is a fixed sequence of fixed-size
//! fields, so a peer that changes it does not report a mismatch — connections
//! simply stop working, and every symptom shows up somewhere else entirely.
//! Upstream `e553f68` changed it, and nothing here noticed, because the
//! fixture suite covers frames and this lives below frames.
//!
//! Two kinds of check, for two different failure modes:
//!
//! - **Vectors** run always. They pin the exact bytes Go signs, so a change on
//!   our side fails here rather than in the field.
//! - **A live exchange** runs when Go is installed, driving
//!   `tests/handshake/main.go` — a transcription of `network/tor.go`'s
//!   handshake over plain TCP — against this implementation in both
//!   directions. That is the part that would catch Go changing again.

use std::process::Stdio;

use libbounce::crypto::{self, DeviceKey};
use libbounce::net::{accept_handshake, dial_handshake};
use tokio::io::{AsyncBufReadExt, BufReader};

/// The vectors `go run . vectors` emits, checked in so they run without Go.
///
/// Regenerate with:
///
/// ```text
/// cd libbounce/tests/handshake && go run . vectors
/// ```
const SEED: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
const ADDRESS: &str = "aoqqpp7tzyil4hlq3umoos6atft6jvrqtosq2xy53sdgiesvgg4bqead";
const CHALLENGE: &str = "a0a1a2a3a4a5a6a7a8a9aaabacadaeafb0b1b2b3b4b5b6b7b8b9babbbcbdbebf";
const CHALLENGE_XOR: &str = "5c5b5a595857565554535251504f4e4d4c4b4a494847464544434241403f3e3d";
const SIGNED_BYTES: &str = "fcfaf8fafcf2f0f2fcfaf8fafce2e0e2fcfaf8fafcf2f0f2fcfaf8fafc828082";
const SIGNATURE: &str = "a6fa4019b4351c6601872051a697e97522e00f0f85efc68c852d804325d8844\
                         89514f2f8219f3b8c23013ed1172b7fb146939c7a5be3cc0af4bdce81233c4204";

fn hex_bytes(value: &str) -> Vec<u8> {
    hex::decode(value).expect("fixture is valid hex")
}

fn seeded_key() -> DeviceKey {
    let seed: [u8; 32] = hex_bytes(SEED).try_into().expect("32 byte seed");
    DeviceKey::from_seed(&seed)
}

#[test]
fn we_derive_the_same_onion_address_as_go() {
    assert_eq!(seeded_key().address(), ADDRESS);
}

#[test]
fn we_combine_the_two_challenge_halves_the_way_go_does() {
    let combined: Vec<u8> = hex_bytes(CHALLENGE)
        .iter()
        .zip(hex_bytes(CHALLENGE_XOR))
        .map(|(left, right)| left ^ right)
        .collect();
    assert_eq!(hex::encode(combined), SIGNED_BYTES);
}

#[test]
fn we_produce_the_signature_go_produces() {
    // Ed25519 is deterministic, so agreeing on the bytes to sign means
    // agreeing on the signature exactly.
    let signature = seeded_key().sign(&hex_bytes(SIGNED_BYTES));
    assert_eq!(hex::encode(signature), SIGNATURE);
}

#[test]
fn we_accept_the_signature_go_produces() {
    assert!(crypto::verify_signature(
        ADDRESS,
        &hex_bytes(SIGNED_BYTES),
        &hex_bytes(SIGNATURE),
    ));
}

#[test]
fn a_signature_over_the_bare_challenge_is_not_accepted() {
    // What both implementations used to do, and the reason the handshake
    // changed: signing the listener's challenge as given makes every address
    // you dial a signing oracle. A peer still doing it must fail, not be
    // quietly tolerated — tolerating it would keep the oracle open for anyone
    // who asks for it.
    let signature = seeded_key().sign(&hex_bytes(CHALLENGE));
    assert!(!crypto::verify_signature(
        ADDRESS,
        &hex_bytes(SIGNED_BYTES),
        &signature,
    ));
}

// ---------------------------------------------------------------------------
// Live exchange with the Go implementation
// ---------------------------------------------------------------------------

/// Where the Go harness lives.
fn harness_directory() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("handshake")
}

/// Whether Go is available to run the harness.
///
/// Absent, the live tests report why and pass: this suite has to build for
/// anyone, and the vectors above still hold the byte format in place.
fn go_available() -> bool {
    match std::process::Command::new("go").arg("version").output() {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

#[tokio::test]
async fn go_can_dial_us() {
    if !go_available() {
        eprintln!("skipping: go is not installed");
        return;
    }

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let child = tokio::process::Command::new("go")
        .args(["run", ".", "dial", &format!("127.0.0.1:{port}")])
        .current_dir(harness_directory())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("go run starts");

    let (mut stream, _) = listener.accept().await.expect("go connects");
    let peer_address = accept_handshake(&mut stream)
        .await
        .expect("the Go client's handshake is accepted");

    let output = child.wait_with_output().await.expect("go exits");
    assert!(
        output.status.success(),
        "go harness failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );

    let claimed = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert_eq!(
        peer_address, claimed,
        "the address we proved must be the one Go was using",
    );
}

#[tokio::test]
async fn we_can_dial_go() {
    if !go_available() {
        eprintln!("skipping: go is not installed");
        return;
    }

    let mut child = tokio::process::Command::new("go")
        .args(["run", ".", "listen", "127.0.0.1:0"])
        .current_dir(harness_directory())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("go run starts");

    // The harness prints the address it bound, so a port is never guessed.
    let stdout = child.stdout.take().expect("piped");
    let mut lines = BufReader::new(stdout).lines();
    let bound = tokio::time::timeout(std::time::Duration::from_secs(120), lines.next_line())
        .await
        .expect("go binds before the timeout")
        .expect("reads a line")
        .expect("go prints the address it bound");

    let key = DeviceKey::generate();
    let expected = key.address();

    let mut stream = tokio::net::TcpStream::connect(&bound).await.expect("connects");
    dial_handshake(&mut stream, &key)
        .await
        .expect("the Go client accepts our handshake");

    let verified = tokio::time::timeout(std::time::Duration::from_secs(30), lines.next_line())
        .await
        .expect("go answers before the timeout")
        .expect("reads a line")
        .expect("go prints the peer address it verified");

    assert_eq!(
        verified, expected,
        "Go must end up with the address we actually hold the key for",
    );
}
