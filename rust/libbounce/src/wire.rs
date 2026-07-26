//! The type-length-value framing used on every Bounce socket.
//!
//! Every frame is a six byte header followed by a MessagePack payload:
//!
//! ```text
//! +--------+--------+--------+--------+--------+--------+ ... +
//! |   frame type    |          payload length           | ... |
//! |     (u16 BE)    |             (u32 BE)              | ... |
//! +--------+--------+--------+--------+--------+--------+ ... +
//! ```
//!
//! The length field occupies the four bytes after the type, so the largest
//! representable payload is 2^32 - 1 bytes. Frames arriving from a device we do
//! not yet know are capped far lower, so that an unauthenticated peer cannot
//! make us allocate an arbitrary buffer.

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::{Error, Result};

/// Total size of the frame header in bytes.
pub const HEADER_SIZE: usize = 6;
/// Size of the frame type field in bytes.
pub const TYPE_SIZE: usize = 2;
/// Size of the payload length field in bytes.
pub const LENGTH_SIZE: usize = HEADER_SIZE - TYPE_SIZE;

/// Largest payload the length field can describe.
pub const MAX_PAYLOAD_SIZE: usize = u32::MAX as usize;

/// Largest payload accepted from a device already in our database.
pub const MAX_PAYLOAD_FROM_KNOWN_DEVICE: usize = MAX_PAYLOAD_SIZE;

/// Largest payload accepted from a device we have never seen. This is enough
/// for an `addUserRequest` carrying a full user structure but small enough that
/// a stranger cannot exhaust memory.
pub const MAX_PAYLOAD_FROM_UNKNOWN_DEVICE: usize = 1024 * 1024;

/// A frame as it appears on the wire: a type tag and an opaque payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawFrame {
    pub frame_type: u16,
    pub payload: Vec<u8>,
}

impl RawFrame {
    pub fn new(frame_type: u16, payload: Vec<u8>) -> Self {
        Self {
            frame_type,
            payload,
        }
    }

    /// Encode this frame, header included, into a new buffer.
    pub fn encode(&self) -> Result<Vec<u8>> {
        if self.payload.len() > MAX_PAYLOAD_SIZE {
            return Err(Error::PayloadTooLarge(self.payload.len()));
        }
        let mut out = Vec::with_capacity(HEADER_SIZE + self.payload.len());
        out.extend_from_slice(&self.frame_type.to_be_bytes());
        out.extend_from_slice(&(self.payload.len() as u32).to_be_bytes());
        out.extend_from_slice(&self.payload);
        Ok(out)
    }

    /// Decode a frame from a complete in-memory buffer.
    ///
    /// Returns the frame and the number of bytes consumed, or `Ok(None)` if the
    /// buffer does not yet hold a whole frame.
    pub fn decode(buf: &[u8], device_is_known: bool) -> Result<Option<(RawFrame, usize)>> {
        if buf.len() < HEADER_SIZE {
            return Ok(None);
        }
        let frame_type = u16::from_be_bytes([buf[0], buf[1]]);
        let payload_size = u32::from_be_bytes([buf[2], buf[3], buf[4], buf[5]]);
        check_payload_size(payload_size, device_is_known)?;

        let total = HEADER_SIZE + payload_size as usize;
        if buf.len() < total {
            return Ok(None);
        }
        Ok(Some((
            RawFrame {
                frame_type,
                payload: buf[HEADER_SIZE..total].to_vec(),
            },
            total,
        )))
    }
}

fn check_payload_size(payload_size: u32, device_is_known: bool) -> Result<()> {
    let limit = if device_is_known {
        MAX_PAYLOAD_FROM_KNOWN_DEVICE
    } else {
        MAX_PAYLOAD_FROM_UNKNOWN_DEVICE
    };
    if payload_size as usize > limit {
        return Err(if device_is_known {
            Error::KnownDeviceFrameTooLarge(payload_size)
        } else {
            Error::UnknownDeviceFrameTooLarge(payload_size)
        });
    }
    Ok(())
}

/// Read one frame from an async stream.
///
/// `device_is_known` selects which payload size limit to enforce. Callers pass
/// `false` until the peer's address has been resolved to a device in the
/// database.
pub async fn read_frame<R>(reader: &mut R, device_is_known: bool) -> Result<RawFrame>
where
    R: AsyncRead + Unpin,
{
    let mut header = [0u8; HEADER_SIZE];
    reader.read_exact(&mut header).await?;

    let frame_type = u16::from_be_bytes([header[0], header[1]]);
    let payload_size = u32::from_be_bytes([header[2], header[3], header[4], header[5]]);
    check_payload_size(payload_size, device_is_known)?;

    let mut payload = vec![0u8; payload_size as usize];
    reader.read_exact(&mut payload).await?;

    Ok(RawFrame {
        frame_type,
        payload,
    })
}

/// Write one frame to an async stream.
pub async fn write_frame<W>(writer: &mut W, frame: &RawFrame) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let encoded = frame.encode()?;
    writer.write_all(&encoded).await?;
    writer.flush().await?;
    Ok(())
}

/// Read exactly `size` bytes, used by the transport handshake which predates
/// the framing layer.
pub async fn read_exact<R>(reader: &mut R, size: usize) -> Result<Vec<u8>>
where
    R: AsyncRead + Unpin,
{
    let mut buf = vec![0u8; size];
    reader.read_exact(&mut buf).await?;
    Ok(buf)
}

/// Write a raw buffer, used by the transport handshake.
pub async fn write_all<W>(writer: &mut W, payload: &[u8]) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    writer.write_all(payload).await?;
    writer.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_layout_matches_go() {
        let frame = RawFrame::new(0x0102, vec![0xaa, 0xbb, 0xcc]);
        let encoded = frame.encode().unwrap();
        assert_eq!(
            encoded,
            vec![0x01, 0x02, 0x00, 0x00, 0x00, 0x03, 0xaa, 0xbb, 0xcc]
        );
    }

    #[test]
    fn empty_payload_encodes_to_bare_header() {
        let encoded = RawFrame::new(6, vec![]).encode().unwrap();
        assert_eq!(encoded, vec![0x00, 0x06, 0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn decode_round_trips() {
        let frame = RawFrame::new(13, b"some msgpack".to_vec());
        let encoded = frame.encode().unwrap();
        let (decoded, used) = RawFrame::decode(&encoded, true).unwrap().unwrap();
        assert_eq!(decoded, frame);
        assert_eq!(used, encoded.len());
    }

    #[test]
    fn decode_reports_incomplete_buffers() {
        let encoded = RawFrame::new(13, b"payload".to_vec()).encode().unwrap();
        assert!(RawFrame::decode(&encoded[..3], true).unwrap().is_none());
        assert!(RawFrame::decode(&encoded[..8], true).unwrap().is_none());
    }

    #[test]
    fn unknown_devices_are_capped_at_one_mib() {
        let mut header = Vec::new();
        header.extend_from_slice(&0u16.to_be_bytes());
        header.extend_from_slice(&((MAX_PAYLOAD_FROM_UNKNOWN_DEVICE as u32) + 1).to_be_bytes());

        assert!(matches!(
            RawFrame::decode(&header, false),
            Err(Error::UnknownDeviceFrameTooLarge(_))
        ));
        // The same header from a known device is allowed through.
        assert!(RawFrame::decode(&header, true).unwrap().is_none());
    }

    #[tokio::test]
    async fn async_round_trip() {
        let frame = RawFrame::new(42, vec![1, 2, 3, 4, 5]);
        let mut buf = Vec::new();
        write_frame(&mut buf, &frame).await.unwrap();

        let mut cursor = std::io::Cursor::new(buf);
        let decoded = read_frame(&mut cursor, true).await.unwrap();
        assert_eq!(decoded, frame);
    }

    #[tokio::test]
    async fn async_read_rejects_oversized_unknown_frame() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&7u16.to_be_bytes());
        buf.extend_from_slice(&(2u32 * 1024 * 1024).to_be_bytes());

        let mut cursor = std::io::Cursor::new(buf);
        assert!(matches!(
            read_frame(&mut cursor, false).await,
            Err(Error::UnknownDeviceFrameTooLarge(_))
        ));
    }
}
