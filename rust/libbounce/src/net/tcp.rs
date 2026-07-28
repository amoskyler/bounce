//! A plain TCP transport, for tests and local development.
//!
//! Standing up two hidden services per test would make the suite take minutes
//! instead of milliseconds. Addresses here are still real onion addresses
//! derived from real keys and the handshake is the real one, so everything
//! above this layer behaves identically — only the routing differs.
//!
//! It provides no metadata protection whatsoever and is not for production use.
//! Production is [`super::tor`].

use std::sync::Arc;

use tokio::net::{TcpListener, TcpStream};

use super::{accept_handshake, dial_handshake, Network, PeerConnection};
use crate::crypto::DeviceKey;
use crate::error::{Error, Result};

/// Resolves an onion address to a socket address.
///
/// Tor does this through the hidden service directory; over plain TCP something
/// has to supply the mapping.
pub trait PeerDirectory: Send + Sync {
    fn resolve(&self, address: &str) -> Option<std::net::SocketAddr>;
}

/// A directory backed by an in-memory table, optionally mirrored to a file.
///
/// Tests keep it in memory: each has its own instances and its own directory.
///
/// Running two clients on one machine is different — they are separate
/// processes, so an in-memory table would leave each unable to resolve the
/// other and every dial would fail with "no route to". Pointing both at the
/// same file gives them somewhere to publish and look each other up, standing
/// in for the hidden service directory Tor would provide.
#[derive(Default)]
pub struct StaticDirectory {
    entries: std::sync::Mutex<std::collections::HashMap<String, std::net::SocketAddr>>,
    /// Where entries are shared between processes, if anywhere.
    shared: Option<std::path::PathBuf>,
}

impl StaticDirectory {
    pub fn new() -> Self {
        Self::default()
    }

    /// A directory shared with every other process pointing at the same file.
    ///
    /// Development only: it publishes which local port each device is on, in
    /// the clear.
    pub fn shared_at(path: impl Into<std::path::PathBuf>) -> Self {
        StaticDirectory {
            entries: std::sync::Mutex::new(std::collections::HashMap::new()),
            shared: Some(path.into()),
        }
    }

    pub fn insert(&self, address: String, socket: std::net::SocketAddr) {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(address.clone(), socket);

        // Publish for other processes. A failure here only means peers on this
        // machine cannot find us, so it is logged rather than propagated.
        if let Some(path) = &self.shared {
            let mut published = Self::read_file(path);
            published.insert(address, socket.to_string());

            match serde_json::to_vec_pretty(&published) {
                Ok(encoded) => {
                    if let Err(error) = std::fs::write(path, encoded) {
                        tracing::warn!(%error, path = %path.display(), "could not publish to the shared peer directory");
                    }
                }
                Err(error) => tracing::warn!(%error, "could not encode the shared peer directory"),
            }
        }
    }

    fn read_file(path: &std::path::Path) -> std::collections::BTreeMap<String, String> {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }
}

impl PeerDirectory for StaticDirectory {
    fn resolve(&self, address: &str) -> Option<std::net::SocketAddr> {
        if let Some(socket) = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(address)
            .copied()
        {
            return Some(socket);
        }

        // Re-read rather than caching: the peer may have started after us, or
        // restarted onto a different port.
        let path = self.shared.as_ref()?;
        Self::read_file(path).get(address)?.parse().ok()
    }
}

/// A TCP transport.
pub struct TcpNetwork {
    key: DeviceKey,
    listener: TcpListener,
    directory: Arc<StaticDirectory>,
}

impl TcpNetwork {
    /// Bind a listener and register this device in the directory.
    pub async fn bind(key: DeviceKey, directory: Arc<StaticDirectory>) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let local = listener.local_addr()?;
        directory.insert(key.address(), local);

        Ok(TcpNetwork {
            key,
            listener,
            directory,
        })
    }

    /// The socket this instance is listening on.
    pub fn local_socket(&self) -> Result<std::net::SocketAddr> {
        Ok(self.listener.local_addr()?)
    }

    /// This instance's signing key.
    pub fn key(&self) -> &DeviceKey {
        &self.key
    }
}

impl Network for TcpNetwork {
    type Stream = TcpStream;

    fn address(&self) -> String {
        self.key.address()
    }

    async fn accept(&self) -> Result<PeerConnection<TcpStream>> {
        let (mut stream, _) = self.listener.accept().await?;
        let peer_address = accept_handshake(&mut stream).await?;

        Ok(PeerConnection {
            peer_address,
            local_address: self.key.address(),
            stream,
        })
    }

    async fn dial(&self, address: &str) -> Result<PeerConnection<TcpStream>> {
        let socket = self
            .directory
            .resolve(address)
            .ok_or_else(|| Error::Network(format!("no route to {address}")))?;

        let mut stream = TcpStream::connect(socket).await?;
        dial_handshake(&mut stream, &self.key).await?;

        Ok(PeerConnection {
            peer_address: address.to_string(),
            local_address: self.key.address(),
            stream,
        })
    }

    fn sign(&self, data: &[u8]) -> Vec<u8> {
        self.key.sign(data).to_vec()
    }

    async fn shutdown(&self) {
        // Dropping the listener suffices; there is no session to tear down.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{self, RawFrame};

    #[tokio::test]
    async fn two_instances_exchange_frames_over_tcp() {
        let directory = Arc::new(StaticDirectory::new());

        let alice = TcpNetwork::bind(DeviceKey::generate(), directory.clone())
            .await
            .unwrap();
        let bob = TcpNetwork::bind(DeviceKey::generate(), directory.clone())
            .await
            .unwrap();

        let alice_address = alice.address();
        let bob_address = bob.address();

        let listener = tokio::spawn(async move {
            let mut connection = bob.accept().await.unwrap();
            let frame = wire::read_frame(&mut connection.stream, true).await.unwrap();
            (connection.peer_address, frame)
        });

        let mut connection = alice.dial(&bob_address).await.unwrap();
        assert_eq!(connection.peer_address, bob_address);

        let sent = RawFrame::new(
            crate::types::FrameType::DirectMessage.as_u16(),
            b"payload".to_vec(),
        );
        wire::write_frame(&mut connection.stream, &sent).await.unwrap();

        let (observed_peer, received) = listener.await.unwrap();

        // Bob learned Alice's real address from the handshake, not from
        // anything Alice asserted inside the frame.
        assert_eq!(observed_peer, alice_address);
        assert_eq!(received, sent);
    }

    #[tokio::test]
    async fn dialing_an_unroutable_address_fails_cleanly() {
        let directory = Arc::new(StaticDirectory::new());
        let alice = TcpNetwork::bind(DeviceKey::generate(), directory)
            .await
            .unwrap();

        let unknown = DeviceKey::generate().address();
        assert!(matches!(alice.dial(&unknown).await, Err(Error::Network(_))));
    }

    #[tokio::test]
    async fn the_address_is_derived_from_the_key_not_the_listener() {
        let key = DeviceKey::generate();
        let expected = key.address();

        let network = TcpNetwork::bind(key, Arc::new(StaticDirectory::new()))
            .await
            .unwrap();

        assert_eq!(network.address(), expected);
        assert_eq!(network.address().len(), crate::onion::ONION_ADDRESS_LENGTH);
    }
}
