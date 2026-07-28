//! The Tor transport: an in-process onion service, via arti.
//!
//! This is what makes Bounce Bounce. Every instance is a v3 onion service and
//! every connection between devices is a Tor circuit, so an observer watching
//! the network cannot tell who is talking to whom.
//!
//! [arti] is the Tor Project's own Rust implementation — not a binding to C
//! tor, a reimplementation. That removes the cgo dependency the Go client
//! carries through go-libtor, which is also what makes its Windows build need
//! a mingw cross-compiler.
//!
//! [arti]: https://gitlab.torproject.org/tpo/core/arti
//!
//! ## The device key *is* the onion identity
//!
//! Bounce's whole identity model rests on a device's address being the
//! encoding of its public key, so the onion service must come up under the key
//! Bounce already persists — not one arti generates.
//!
//! That works because arti's `HsIdKeypair` stores an *expanded* secret key
//! "for compatibility with the C tor implementation, and in order to support
//! custom-generated addresses", which is exactly the 64-byte form
//! [`DeviceKey::to_expanded_bytes`] produces. A device keeps its address
//! across the port from Go, and across restarts.
//!
//! [`start`] verifies this rather than trusting it: if the address arti
//! publishes is not the address derived from our key, it fails loudly instead
//! of quietly becoming a different device.
//!
//! ## Two arti behaviours worth knowing about
//!
//! **The keystore is written to.** `launch_onion_service_with_hsid` is not
//! ephemeral — internally it inserts the supplied key into the primary
//! keystore, which by default is on disk. So the identity key ends up in
//! arti's state directory as well as Bounce's. That directory must be treated
//! as secret-bearing.
//!
//! **That insert refuses to overwrite.** It passes `overwrite = false`, so the
//! second launch under the same nickname fails with `KeyAlreadyExists`. Bounce
//! clears the service's keystore directory before launching, which both fixes
//! the failure and guarantees the running service uses *our* key. Falling back
//! to the plain `launch_onion_service` on error would be the alternative, and
//! is worse: if the stored key ever diverged from ours, the service would come
//! up under the wrong address without complaint.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use arti_client::{DataStream, TorClient, TorClientConfig};
use futures::StreamExt;
use safelog::DisplayRedacted as _;
use tokio::sync::{mpsc, Mutex, Semaphore};
use tor_cell::relaycell::msg::Connected;
use tor_hscrypto::pk::{HsIdKey, HsIdKeypair};
use tor_hsservice::config::OnionServiceConfigBuilder;
use tor_hsservice::{HsNickname, RunningOnionService, StreamRequest};
use tor_llcrypto::pk::ed25519::ExpandedKeypair;
use tor_proto::stream::IncomingStreamRequest;
use tor_rtcompat::tokio::TokioRustlsRuntime;

use super::{
    accept_handshake, dial_handshake, Network, PeerConnection, BOUNCE_PORT,
};
use crate::crypto::DeviceKey;
use crate::error::{Error, Result};

/// The nickname arti files this service's keys and state under.
const SERVICE_NICKNAME: &str = "bounce";

/// How many inbound handshakes may be in flight at once.
///
/// Handshakes run concurrently so one slow peer cannot stall the accept loop,
/// but they are bounded so a flood of half-open circuits cannot exhaust memory.
const MAX_CONCURRENT_HANDSHAKES: usize = 32;

/// Depth of the queue of handshaken-but-not-yet-collected connections.
const ACCEPT_QUEUE_DEPTH: usize = 64;

/// A running Bounce onion service.
pub struct TorNetwork {
    key: DeviceKey,
    client: Arc<TorClient<TokioRustlsRuntime>>,
    /// Kept alive for as long as the network is: dropping it takes the service
    /// down.
    _service: Arc<RunningOnionService>,
    /// Connections that have completed the handshake.
    accepted: Mutex<mpsc::Receiver<PeerConnection<DataStream>>>,
    address: String,
}

impl TorNetwork {
    /// Bootstrap Tor and publish this device's onion service.
    ///
    /// `state_dir` holds arti's persistent state — including, unavoidably, a
    /// copy of the identity key — and `cache_dir` its directory cache. Both
    /// are created if absent.
    ///
    /// This blocks until Tor has bootstrapped, which on a cold start is
    /// typically tens of seconds.
    pub async fn start(key: DeviceKey, state_dir: &Path, cache_dir: &Path) -> Result<Self> {
        install_crypto_provider()?;

        std::fs::create_dir_all(state_dir)?;
        std::fs::create_dir_all(cache_dir)?;
        restrict_permissions(state_dir)?;

        let expected_address = key.address();

        let runtime = TokioRustlsRuntime::current()
            .map_err(|error| Error::Network(format!("no tokio runtime for arti: {error}")))?;

        let config: TorClientConfig = arti_client::config::TorClientConfigBuilder::from_directories(
            state_dir,
            cache_dir,
        )
        .build()
        .map_err(|error| Error::Network(format!("invalid Tor configuration: {error}")))?;

        let client = TorClient::with_runtime(runtime)
            .config(config)
            .create_unbootstrapped_async()
            .await
            .map_err(|error| Error::Network(format!("could not create the Tor client: {error}")))?;

        client
            .bootstrap()
            .await
            .map_err(|error| Error::Network(format!("could not bootstrap Tor: {error}")))?;

        let nickname: HsNickname = SERVICE_NICKNAME
            .parse()
            .map_err(|error| Error::Network(format!("invalid service nickname: {error}")))?;

        // See the module documentation: the launch below inserts our key with
        // `overwrite = false`, so anything already filed under this nickname
        // has to go first.
        clear_service_keystore(state_dir, SERVICE_NICKNAME)?;

        let service_config = OnionServiceConfigBuilder::default()
            .nickname(nickname)
            .build()
            .map_err(|error| Error::Network(format!("invalid onion service config: {error}")))?;

        let launched = client
            .launch_onion_service_with_hsid(service_config, hs_id_keypair(&key)?)
            .map_err(|error| Error::Network(format!("could not launch the onion service: {error}")))?;

        let (service, rend_requests) = launched.ok_or_else(|| {
            Error::Network("the onion service is disabled in configuration".into())
        })?;

        let address = onion_address(&key)?;

        // The safety net against arti having used a different key than ours:
        // an address mismatch means this device would appear to its contacts
        // as a stranger.
        if address != expected_address {
            return Err(Error::Network(format!(
                "onion service came up under {address} but this device's key is {expected_address}"
            )));
        }

        // Flatten rendezvous requests into stream requests, then hand each to a
        // task that completes the handshake. Doing the handshakes here rather
        // than in `accept` keeps one slow peer from stalling every other.
        let (sender, receiver) = mpsc::channel(ACCEPT_QUEUE_DEPTH);
        let local_address = address.clone();
        tokio::spawn(async move {
            let mut streams = Box::pin(tor_hsservice::handle_rend_requests(rend_requests));
            let permits = Arc::new(Semaphore::new(MAX_CONCURRENT_HANDSHAKES));

            while let Some(request) = streams.next().await {
                let Ok(permit) = Arc::clone(&permits).acquire_owned().await else {
                    break;
                };
                let sender = sender.clone();
                let local_address = local_address.clone();

                tokio::spawn(async move {
                    let _permit = permit;
                    if let Some(connection) = handshake_inbound(request, local_address).await {
                        // A full queue means the engine is not collecting;
                        // dropping is correct, the peer will redial.
                        let _ = sender.try_send(connection);
                    }
                });
            }
        });

        Ok(TorNetwork {
            key,
            client,
            _service: service,
            accepted: Mutex::new(receiver),
            address,
        })
    }

    /// The onion address this device publishes.
    pub fn onion_address(&self) -> &str {
        &self.address
    }
}

impl Network for TorNetwork {
    type Stream = DataStream;

    fn address(&self) -> String {
        self.address.clone()
    }

    async fn accept(&self) -> Result<PeerConnection<DataStream>> {
        let mut queue = self.accepted.lock().await;
        queue
            .recv()
            .await
            .ok_or_else(|| Error::Network("the onion service stopped accepting".into()))
    }

    async fn dial(&self, address: &str) -> Result<PeerConnection<DataStream>> {
        let target = format!("{address}.onion:{BOUNCE_PORT}");

        let mut stream = self
            .client
            .connect(&target)
            .await
            .map_err(|error| Error::Network(format!("could not reach {address}: {error}")))?;

        dial_handshake(&mut stream, &self.key).await?;

        Ok(PeerConnection {
            peer_address: address.to_string(),
            local_address: self.address.clone(),
            stream,
        })
    }

    fn sign(&self, data: &[u8]) -> Vec<u8> {
        self.key.sign(data).to_vec()
    }

    async fn shutdown(&self) {
        // Dropping the client and the service handle tears down the circuits.
    }
}

/// Accept one inbound stream and complete the Bounce handshake on it.
///
/// Returns `None` for anything that is not a well-formed Bounce connection, so
/// the accept loop is never taken down by a peer's behaviour.
async fn handshake_inbound(
    request: StreamRequest,
    local_address: String,
) -> Option<PeerConnection<DataStream>> {
    // Only BEGIN on our port. Answering anything else — or answering a
    // different port differently — would make the service fingerprintable.
    match request.request() {
        IncomingStreamRequest::Begin(begin) if begin.port() == BOUNCE_PORT => {}
        _ => {
            let _ = request.shutdown_circuit();
            return None;
        }
    }

    let mut stream = match request.accept(Connected::new_empty()).await {
        Ok(stream) => stream,
        Err(error) => {
            tracing::debug!(%error, "could not accept an inbound Tor stream");
            return None;
        }
    };

    // A v3 onion service learns nothing about who connected to it, so this
    // handshake is the only thing that establishes the peer's identity.
    match accept_handshake(&mut stream).await {
        Ok(peer_address) => Some(PeerConnection {
            peer_address,
            local_address,
            stream,
        }),
        Err(error) => {
            tracing::debug!(%error, "inbound Tor connection failed the handshake");
            None
        }
    }
}

/// Build arti's identity keypair from Bounce's device key.
fn hs_id_keypair(key: &DeviceKey) -> Result<HsIdKeypair> {
    let expanded = ExpandedKeypair::from_secret_key_bytes(key.to_expanded_bytes()).ok_or_else(
        || Error::InvalidKey("device key is not a valid expanded Ed25519 secret".into()),
    )?;
    Ok(HsIdKeypair::from(expanded))
}

/// The onion address for a device key, as arti computes it.
///
/// Derived through arti's own types rather than [`crate::onion`], so that the
/// startup check compares two independent derivations instead of one against
/// itself.
fn onion_address(key: &DeviceKey) -> Result<String> {
    let expanded = ExpandedKeypair::from_secret_key_bytes(key.to_expanded_bytes()).ok_or_else(
        || Error::InvalidKey("device key is not a valid expanded Ed25519 secret".into()),
    )?;

    let hs_id = HsIdKey::from(*expanded.public()).id();

    // `HsId` redacts itself when displayed, since an onion address is
    // identifying. Ours is not a secret from us. The rendered value is bound
    // to a local because the displayer borrows `hs_id`.
    let rendered = hs_id.display_unredacted().to_string();
    Ok(rendered.trim_end_matches(".onion").to_string())
}

/// Remove any key arti has already filed under this service nickname.
///
/// See the module documentation: without this, every launch after the first
/// fails, because arti refuses to overwrite an existing identity key.
fn clear_service_keystore(state_dir: &Path, nickname: &str) -> Result<()> {
    let service_keys: PathBuf = state_dir.join("keystore").join("hss").join(nickname);

    if service_keys.exists() {
        std::fs::remove_dir_all(&service_keys)?;
        tracing::debug!(path = %service_keys.display(), "cleared the arti keystore for this service");
    }

    Ok(())
}

/// Narrow permissions on the directory arti keeps our identity key in.
#[cfg(unix)]
fn restrict_permissions(directory: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_permissions(_directory: &Path) -> Result<()> {
    // Windows inherits the user profile's ACL, which is already user-only.
    Ok(())
}

/// Select a rustls cryptography provider.
///
/// arti pulls in rustls without choosing one, so without this the first TLS
/// handshake panics with "Could not automatically determine the process-level
/// CryptoProvider". Installing is process-global and one-shot; a second call
/// returning `Err` just means somebody got there first, which is fine.
fn install_crypto_provider() -> Result<()> {
    static INSTALLED: std::sync::Once = std::sync::Once::new();
    INSTALLED.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // These exercise the pure parts — key conversion and address derivation.
    // Anything beyond that needs a live Tor bootstrap, which does not belong in
    // a unit test suite; the TCP transport covers the protocol paths.

    #[test]
    fn arti_derives_the_same_onion_address_as_bounce() {
        // The two derivations are independent: `crate::onion` encodes the
        // public key by hand, while this goes through arti's HsIdKey. They must
        // agree, or the startup check in `start` would be comparing a value
        // against itself and could never catch a real mismatch.
        for _ in 0..16 {
            let key = DeviceKey::generate();
            assert_eq!(onion_address(&key).unwrap(), key.address());
        }
    }

    #[test]
    fn a_seed_key_and_its_expansion_yield_the_same_address() {
        let seed_key = DeviceKey::generate();
        let expanded = DeviceKey::from_expanded_bytes(&seed_key.to_expanded_bytes()).unwrap();

        assert_eq!(onion_address(&seed_key).unwrap(), onion_address(&expanded).unwrap());
    }

    #[test]
    fn the_identity_keypair_round_trips_to_the_expected_public_key() {
        let key = DeviceKey::generate();
        let keypair = hs_id_keypair(&key).unwrap();

        let public = ExpandedKeypair::from(keypair).public().to_bytes();
        assert_eq!(public, key.public_key());
    }

    #[test]
    fn clearing_an_absent_keystore_is_not_an_error() {
        let temp = tempfile::tempdir().unwrap();
        assert!(clear_service_keystore(temp.path(), SERVICE_NICKNAME).is_ok());
    }

    #[test]
    fn clearing_removes_a_previously_stored_key() {
        let temp = tempfile::tempdir().unwrap();
        let service_keys = temp.path().join("keystore").join("hss").join(SERVICE_NICKNAME);
        std::fs::create_dir_all(&service_keys).unwrap();
        std::fs::write(service_keys.join("ks_hs_id.ed25519_expanded_private"), b"stale").unwrap();

        clear_service_keystore(temp.path(), SERVICE_NICKNAME).unwrap();
        assert!(!service_keys.exists());
    }

    #[test]
    fn installing_the_crypto_provider_is_idempotent() {
        assert!(install_crypto_provider().is_ok());
        assert!(install_crypto_provider().is_ok());
    }
}
