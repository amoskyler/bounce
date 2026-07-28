//! Transport.
//!
//! Bounce does not depend on Tor specifically — it depends on any
//! *cryptographically addressed* network, one where an address is a public key
//! and a peer's address can be learned and verified when they connect. Tor v3
//! onion services satisfy that and are what production uses, but nothing above
//! this layer assumes it.
//!
//! - [`tcp`] — a plain TCP transport for tests and local development
//! - [`tor`] — the real thing, an in-process onion service via arti
//!   (behind the `tor` feature)
//!
//! ## The handshake
//!
//! A raw socket would let anyone claim any address, so every connection begins
//! with a challenge-response proving the dialer holds the private key for the
//! address it claims:
//!
//! ```text
//!   listener                              dialer
//!      |------- 32 random bytes ------------->|
//!      |<------ 56 byte onion address --------|
//!      |<------ 32 random bytes --------------|
//!      |<------ 64 byte Ed25519 signature ----|
//!      |  verify against challenge ^ their 32 |
//! ```
//!
//! The listener's own address needs no proving: the dialer had to know it to
//! reach it at all. This is not redundant with Tor — a v3 onion service learns
//! nothing about *who* connected to it, so the handshake is the only thing that
//! establishes the peer's identity. After it, both sides speak the framing in
//! [`crate::wire`].
//!
//! ## Why both sides contribute to the challenge
//!
//! A device's key signs two different things: handshake responses, and the
//! `BLAKE3` digest inside every [`SignedContainer`]. That digest is 32 bytes —
//! exactly the size of a challenge. If the dialer signed the listener's
//! challenge as-is, **any address you dial would be a signing oracle**: a
//! malicious listener could send `BLAKE3(frame)` as the challenge and receive a
//! signature that verifies as a frame authored by your device. It could then
//! put words in your mouth to every one of your contacts — messages, group
//! removals, consensus confirmations — with a fresh signature on every
//! reconnect.
//!
//! So the dialer contributes 32 random bytes of its own and both sides use
//! `challenge XOR dialer_bytes`. The listener has already committed to its
//! challenge by the time those bytes are chosen, so it cannot steer the result
//! anywhere: whatever it sends, the signed message is uniformly random to it.
//! A malicious *dialer* can of course choose the outcome — it picks its half
//! last — but it is signing with its own key, and a signature it could have
//! made anyway is not a capability it gained.
//!
//! [`SignedContainer`]: crate::signed::SignedContainer
//!
//! ## Interoperating with the Go implementation
//!
//! This is Go's handshake, byte for byte (`network/tor.go`, upstream
//! `e553f68`). It has to be: the exchange is fixed-size and unversioned, so
//! there is no room to negotiate and no way to tell a mismatch from a bad
//! signature.
//!
//! This port previously solved the oracle problem differently — the dialer
//! signed `BLAKE3("bounce-handshake-v1" || listener || challenge)` — which was
//! sound but not what Go did, so reaching a Go peer meant opting in to signing
//! the bare challenge and accepting the oracle for the duration. Go now
//! contributes randomness instead, which closes the same hole without anyone
//! having to choose, so both the transcript and the opt-in are gone.
//!

pub mod tcp;

#[cfg(feature = "tor")]
pub mod tor;

pub use tcp::{PeerDirectory, StaticDirectory, TcpNetwork};

#[cfg(feature = "tor")]
pub use tor::TorNetwork;

use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite};

use crate::crypto::{self, DeviceKey};
use crate::error::{Error, Result};
use crate::onion;
use crate::wire;

/// Size of the random challenge issued by the listener.
pub const HANDSHAKE_CHALLENGE_SIZE: usize = 32;
/// Size of an Ed25519 signature.
pub const SIGNATURE_SIZE: usize = 64;

/// How long a peer has to complete the handshake.
///
/// Without this, one peer that connects and then stalls would block the accept
/// loop for every other peer. Tor circuits are slow enough that the limit has
/// to be generous, but it must exist.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

/// The virtual port Bounce listens on. Onion services do their own
/// multiplexing, so the number is arbitrary — it just has to match what peers
/// dial, and the Go implementation uses 80.
pub const BOUNCE_PORT: u16 = 80;

/// Anything that can carry framed Bounce traffic.
///
/// The blanket implementation means transports name their concrete stream type
/// and callers that need to erase it can use `Box<dyn PeerStream>`, which is
/// itself a `PeerStream`.
pub trait PeerStream: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send + ?Sized> PeerStream for T {}

/// A connection to a peer, with the peer's verified address attached.
pub struct PeerConnection<S> {
    /// The peer's onion address, proven during the handshake.
    pub peer_address: String,
    /// Our own address.
    pub local_address: String,
    pub stream: S,
}

impl<S> PeerConnection<S> {
    /// Erase the concrete stream type, so connections from different
    /// transports can share a type.
    pub fn boxed(self) -> PeerConnection<Box<dyn PeerStream>>
    where
        S: PeerStream + 'static,
    {
        PeerConnection {
            peer_address: self.peer_address,
            local_address: self.local_address,
            stream: Box::new(self.stream),
        }
    }
}

/// A network Bounce can run on.
///
/// Implementations must guarantee that [`accept`](Network::accept) only returns
/// connections whose peer address has been cryptographically verified — every
/// authorization decision above this layer trusts that address.
#[allow(async_fn_in_trait)]
pub trait Network: Send + Sync {
    /// The stream type this network produces.
    type Stream: PeerStream;

    /// This device's address. Must be available even while offline, since it is
    /// derived from the private key rather than from a live service.
    fn address(&self) -> String;

    /// Accept an inbound connection, completing the handshake.
    async fn accept(&self) -> Result<PeerConnection<Self::Stream>>;

    /// Dial a peer, completing the handshake.
    async fn dial(&self, address: &str) -> Result<PeerConnection<Self::Stream>>;

    /// Sign data with this device's key.
    fn sign(&self, data: &[u8]) -> Vec<u8>;

    /// Verify a signature made by the device at `address`.
    fn verify(&self, address: &str, data: &[u8], signature: &[u8]) -> bool {
        crypto::verify_signature(address, data, signature)
    }

    /// Stop listening.
    async fn shutdown(&self);
}

/// Run the listener's half of the handshake, returning the peer's proven
/// address.
///
/// Bounded by [`HANDSHAKE_TIMEOUT`]; a peer that goes quiet mid-handshake is
/// dropped rather than held onto.
pub async fn accept_handshake<S>(stream: &mut S) -> Result<String>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    tokio::time::timeout(
        HANDSHAKE_TIMEOUT,
        accept_handshake_inner(stream),
    )
    .await
    .map_err(|_| Error::Network("handshake timed out".into()))?
}

async fn accept_handshake_inner<S>(stream: &mut S) -> Result<String>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let challenge = crypto::random_bytes(HANDSHAKE_CHALLENGE_SIZE);
    wire::write_all(stream, &challenge).await?;

    let address_bytes = wire::read_exact(stream, onion::ONION_ADDRESS_LENGTH).await?;
    let peer_address = String::from_utf8(address_bytes)
        .map_err(|_| Error::Network("peer address is not valid UTF-8".into()))?;

    // Reject a malformed address before spending a signature verification on it.
    if !onion::is_valid_address(&peer_address) {
        return Err(Error::Network(format!(
            "peer presented an invalid address: {peer_address}"
        )));
    }

    // The dialer's half of the challenge, chosen after ours was sent.
    let peer_challenge = wire::read_exact(stream, HANDSHAKE_CHALLENGE_SIZE).await?;
    let signature = wire::read_exact(stream, SIGNATURE_SIZE).await?;

    if !crypto::verify_signature(&peer_address, &xor(&challenge, &peer_challenge), &signature) {
        return Err(Error::Network(format!(
            "handshake signature did not verify against {peer_address}"
        )));
    }

    Ok(peer_address)
}

/// The two halves of a challenge, combined.
///
/// Both are [`HANDSHAKE_CHALLENGE_SIZE`], which is what makes this total; Go
/// treats a length mismatch as unreachable and aborts the process.
fn xor(a: &[u8], b: &[u8]) -> Vec<u8> {
    a.iter().zip(b).map(|(left, right)| left ^ right).collect()
}

/// Run the dialer's half of the handshake.
pub async fn dial_handshake<S>(stream: &mut S, key: &DeviceKey) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    tokio::time::timeout(HANDSHAKE_TIMEOUT, dial_handshake_inner(stream, key))
        .await
        .map_err(|_| Error::Network("handshake timed out".into()))?
}

async fn dial_handshake_inner<S>(stream: &mut S, key: &DeviceKey) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let challenge = wire::read_exact(stream, HANDSHAKE_CHALLENGE_SIZE).await?;

    // Our half, chosen only after theirs has arrived. That ordering is the
    // whole protection: they cannot aim the signed bytes at anything, because
    // they committed to their half before seeing ours.
    let peer_challenge = crypto::random_bytes(HANDSHAKE_CHALLENGE_SIZE);
    let signature = key.sign(&xor(&challenge, &peer_challenge));

    let address = key.address();
    wire::write_all(stream, address.as_bytes()).await?;
    wire::write_all(stream, &peer_challenge).await?;
    wire::write_all(stream, &signature).await?;

    Ok(())
}

/// The transport an instance is actually running on.
///
/// The engine is generic over [`Network`], which is right for the protocol but
/// means each transport produces a different `Engine<_>` type. Applications
/// pick their transport at runtime — Tor normally, TCP for local testing — so
/// this erases the difference behind one type, at the cost of boxing each
/// stream once when the connection is established.
pub enum Transport {
    Tcp(TcpNetwork),
    #[cfg(feature = "tor")]
    Tor(TorNetwork),
}

impl Transport {
    /// Whether this transport provides metadata protection.
    ///
    /// Exposed so an application can refuse to run, or say so plainly, rather
    /// than silently offering none.
    pub fn is_anonymous(&self) -> bool {
        match self {
            Transport::Tcp(_) => false,
            #[cfg(feature = "tor")]
            Transport::Tor(_) => true,
        }
    }

    /// A short name for logs and the interface.
    pub fn name(&self) -> &'static str {
        match self {
            Transport::Tcp(_) => "tcp",
            #[cfg(feature = "tor")]
            Transport::Tor(_) => "tor",
        }
    }
}

impl Network for Transport {
    type Stream = Box<dyn PeerStream>;

    fn address(&self) -> String {
        match self {
            Transport::Tcp(network) => network.address(),
            #[cfg(feature = "tor")]
            Transport::Tor(network) => network.address(),
        }
    }

    async fn accept(&self) -> Result<PeerConnection<Self::Stream>> {
        match self {
            Transport::Tcp(network) => network.accept().await.map(PeerConnection::boxed),
            #[cfg(feature = "tor")]
            Transport::Tor(network) => network.accept().await.map(PeerConnection::boxed),
        }
    }

    async fn dial(&self, address: &str) -> Result<PeerConnection<Self::Stream>> {
        match self {
            Transport::Tcp(network) => network.dial(address).await.map(PeerConnection::boxed),
            #[cfg(feature = "tor")]
            Transport::Tor(network) => network.dial(address).await.map(PeerConnection::boxed),
        }
    }

    fn sign(&self, data: &[u8]) -> Vec<u8> {
        match self {
            Transport::Tcp(network) => network.sign(data),
            #[cfg(feature = "tor")]
            Transport::Tor(network) => network.sign(data),
        }
    }

    async fn shutdown(&self) {
        match self {
            Transport::Tcp(network) => network.shutdown().await,
            #[cfg(feature = "tor")]
            Transport::Tor(network) => network.shutdown().await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn a_transport_forwards_to_the_network_it_wraps() {
        let key = DeviceKey::generate();
        let expected = key.address();

        let transport = Transport::Tcp(
            TcpNetwork::bind(key, Arc::new(StaticDirectory::new()))
                .await
                .unwrap(),
        );

        assert_eq!(transport.address(), expected);
        assert_eq!(transport.name(), "tcp");
        assert!(
            !transport.is_anonymous(),
            "TCP must never claim to protect metadata"
        );
    }

    #[tokio::test]
    async fn boxed_transports_still_carry_frames() {
        use crate::wire::{self, RawFrame};

        let directory = Arc::new(StaticDirectory::new());
        let alice = Transport::Tcp(
            TcpNetwork::bind(DeviceKey::generate(), Arc::clone(&directory))
                .await
                .unwrap(),
        );
        let bob = Transport::Tcp(
            TcpNetwork::bind(DeviceKey::generate(), Arc::clone(&directory))
                .await
                .unwrap(),
        );
        let bob_address = bob.address();

        let listener = tokio::spawn(async move {
            let mut connection = bob.accept().await.unwrap();
            wire::read_frame(&mut connection.stream, true).await.unwrap()
        });

        let mut connection = alice.dial(&bob_address).await.unwrap();
        let sent = RawFrame::new(0, b"through a boxed stream".to_vec());
        wire::write_frame(&mut connection.stream, &sent).await.unwrap();

        assert_eq!(listener.await.unwrap(), sent);
    }

    /// Play the dialer's half by hand, so a test can vary one step of it.
    async fn dial_by_hand<S>(stream: &mut S, key: &DeviceKey, peer_challenge: &[u8]) -> Vec<u8>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let challenge = wire::read_exact(stream, HANDSHAKE_CHALLENGE_SIZE).await.unwrap();
        let signature = key.sign(&xor(&challenge, peer_challenge));
        wire::write_all(stream, key.address().as_bytes()).await.unwrap();
        wire::write_all(stream, peer_challenge).await.unwrap();
        wire::write_all(stream, &signature).await.unwrap();
        signature.to_vec()
    }

    #[tokio::test]
    async fn a_handshake_establishes_the_dialers_verified_address() {
        let (mut listener_side, mut dialer_side) = tokio::io::duplex(4096);

        let dialer_key = DeviceKey::generate();
        let expected = dialer_key.address();

        let dialer = tokio::spawn(async move {
            dial_handshake(&mut dialer_side, &dialer_key).await.unwrap();
        });

        let peer_address = accept_handshake(&mut listener_side)
            .await
            .expect("handshake succeeds");
        dialer.await.unwrap();

        assert_eq!(peer_address, expected);
    }

    #[tokio::test]
    async fn the_wire_is_challenge_address_challenge_signature() {
        // The exchange is fixed-size and unversioned, so a peer cannot tell a
        // layout change from a bad signature — it just stops connecting. This
        // pins the layout Go reads (`network/tor.go`, upstream `e553f68`).
        let (mut listener_side, mut dialer_side) = tokio::io::duplex(4096);

        let dialer_key = DeviceKey::generate();
        let dialer = tokio::spawn(async move {
            dial_handshake(&mut dialer_side, &dialer_key).await.unwrap();
            dialer_key
        });

        // Act as the listener by hand: send the challenge, then read the three
        // fields the dialer owes us, in order.
        let challenge = crypto::random_bytes(HANDSHAKE_CHALLENGE_SIZE);
        wire::write_all(&mut listener_side, &challenge).await.unwrap();

        let address = wire::read_exact(&mut listener_side, onion::ONION_ADDRESS_LENGTH)
            .await
            .unwrap();
        let peer_challenge = wire::read_exact(&mut listener_side, HANDSHAKE_CHALLENGE_SIZE)
            .await
            .unwrap();
        let signature = wire::read_exact(&mut listener_side, SIGNATURE_SIZE)
            .await
            .unwrap();

        let key = dialer.await.unwrap();
        assert_eq!(String::from_utf8(address).unwrap(), key.address());
        assert_ne!(
            peer_challenge, challenge,
            "the dialer's half must be its own, not an echo",
        );
        assert!(crypto::verify_signature(
            &key.address(),
            &xor(&challenge, &peer_challenge),
            &signature,
        ));
    }

    #[tokio::test]
    async fn a_peer_that_cannot_sign_for_its_address_is_rejected() {
        let (mut listener_side, mut dialer_side) = tokio::io::duplex(4096);

        // Claim someone else's address while signing with our own key.
        let victim_address = DeviceKey::generate().address();
        let attacker_key = DeviceKey::generate();

        tokio::spawn(async move {
            let challenge = wire::read_exact(&mut dialer_side, HANDSHAKE_CHALLENGE_SIZE)
                .await
                .unwrap();
            let peer_challenge = vec![7u8; HANDSHAKE_CHALLENGE_SIZE];
            let signature = attacker_key.sign(&xor(&challenge, &peer_challenge));
            wire::write_all(&mut dialer_side, victim_address.as_bytes()).await.unwrap();
            wire::write_all(&mut dialer_side, &peer_challenge).await.unwrap();
            wire::write_all(&mut dialer_side, &signature).await.unwrap();
        });

        assert!(
            matches!(accept_handshake(&mut listener_side).await, Err(Error::Network(_))),
            "impersonating an address must fail the handshake"
        );
    }

    #[tokio::test]
    async fn a_malformed_address_is_rejected_before_verification() {
        let (mut listener_side, mut dialer_side) = tokio::io::duplex(4096);

        tokio::spawn(async move {
            let _ = wire::read_exact(&mut dialer_side, HANDSHAKE_CHALLENGE_SIZE).await;
            // The right length, but not a valid onion address.
            let junk = vec![b'!'; onion::ONION_ADDRESS_LENGTH];
            let _ = wire::write_all(&mut dialer_side, &junk).await;
            let _ = wire::write_all(&mut dialer_side, &[0u8; HANDSHAKE_CHALLENGE_SIZE]).await;
            let _ = wire::write_all(&mut dialer_side, &[0u8; SIGNATURE_SIZE]).await;
        });

        assert!(accept_handshake(&mut listener_side).await.is_err());
    }

    #[tokio::test]
    async fn a_captured_signature_cannot_be_replayed() {
        let dialer_key = DeviceKey::generate();

        // Capture a signature from a legitimate session, along with the half
        // of the challenge the dialer contributed to it.
        let (mut listener_side, mut dialer_side) = tokio::io::duplex(4096);
        let captured_key = dialer_key.clone();
        let peer_challenge = vec![3u8; HANDSHAKE_CHALLENGE_SIZE];
        let replayed_challenge = peer_challenge.clone();
        let capture = tokio::spawn(async move {
            dial_by_hand(&mut dialer_side, &captured_key, &peer_challenge).await
        });
        accept_handshake(&mut listener_side).await.unwrap();
        let captured_signature = capture.await.unwrap();

        // Replaying both fails, because each session issues a fresh challenge
        // and the dialer's half cannot cancel one it has not seen.
        let (mut listener_side, mut dialer_side) = tokio::io::duplex(4096);
        let address = dialer_key.address();
        tokio::spawn(async move {
            let _ = wire::read_exact(&mut dialer_side, HANDSHAKE_CHALLENGE_SIZE).await;
            let _ = wire::write_all(&mut dialer_side, address.as_bytes()).await;
            let _ = wire::write_all(&mut dialer_side, &replayed_challenge).await;
            let _ = wire::write_all(&mut dialer_side, &captured_signature).await;
        });

        assert!(accept_handshake(&mut listener_side).await.is_err());
    }

    #[tokio::test]
    async fn a_dialed_peer_cannot_use_the_handshake_as_a_signing_oracle() {
        // The attack this exists to prevent: a malicious listener sends
        // BLAKE3(frame) as the challenge, hoping the response doubles as a
        // frame signature from the dialer's device. The dialer's own half of
        // the challenge is what defeats it — the listener has already
        // committed by the time that half is chosen.
        use crate::signed::SignedContainer;

        let victim_key = DeviceKey::generate();
        let victim_address = victim_key.address();

        // A frame the attacker would like the victim to appear to have written.
        let forged_payload = b"a message the victim never wrote".to_vec();
        let digest = crypto::hash(&forged_payload);
        assert_eq!(
            digest.len(),
            HANDSHAKE_CHALLENGE_SIZE,
            "the collision this guards against depends on the sizes matching"
        );

        let (mut listener_side, mut dialer_side) = tokio::io::duplex(4096);

        let attacker = tokio::spawn(async move {
            // Send the digest where a random challenge belongs.
            wire::write_all(&mut listener_side, &digest).await.unwrap();
            let _ = wire::read_exact(&mut listener_side, onion::ONION_ADDRESS_LENGTH).await;
            let _ = wire::read_exact(&mut listener_side, HANDSHAKE_CHALLENGE_SIZE).await;
            wire::read_exact(&mut listener_side, SIGNATURE_SIZE).await.unwrap()
        });

        dial_handshake(&mut dialer_side, &victim_key).await.unwrap();
        let harvested = attacker.await.unwrap();

        let forged = SignedContainer {
            signer: victim_address.clone(),
            payload: forged_payload,
            signature: harvested,
        };
        assert!(
            !forged.is_valid(),
            "a dialed peer must not be able to harvest a usable frame signature"
        );
    }

    #[tokio::test]
    async fn a_peer_that_stalls_mid_handshake_does_not_block_forever() {
        tokio::time::pause();

        let (mut listener_side, mut dialer_side) = tokio::io::duplex(4096);

        // Read the challenge, then never answer.
        tokio::spawn(async move {
            let _ = wire::read_exact(&mut dialer_side, HANDSHAKE_CHALLENGE_SIZE).await;
            std::future::pending::<()>().await;
        });

        let handshake = tokio::spawn(async move { accept_handshake(&mut listener_side).await });
        tokio::time::advance(HANDSHAKE_TIMEOUT + Duration::from_secs(1)).await;

        let result = handshake.await.unwrap();
        assert!(
            matches!(result, Err(Error::Network(ref message)) if message.contains("timed out")),
            "a stalled handshake must time out, got {result:?}"
        );
    }

    #[test]
    fn a_boxed_stream_is_still_a_peer_stream() {
        // The engine needs one connection type across transports, which relies
        // on `Box<dyn PeerStream>` itself satisfying the trait.
        fn assert_peer_stream<T: PeerStream>() {}
        assert_peer_stream::<Box<dyn PeerStream>>();
        assert_peer_stream::<tokio::net::TcpStream>();
    }
}
