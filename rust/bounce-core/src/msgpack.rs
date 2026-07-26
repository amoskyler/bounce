//! Go-compatible MessagePack encoding.
//!
//! The Go implementation encodes frame payloads with `Basekick-Labs/msgpack/v6`,
//! whose defaults differ from `rmp-serde`'s in two ways that matter:
//!
//! | | Go | `rmp-serde` default |
//! |---|---|---|
//! | structs | map keyed by the exact Go field name | array of values |
//! | `[]byte` | `bin` | array of integers |
//!
//! So everything here goes through [`to_vec`], which turns on struct-map mode,
//! and every byte-slice field in [`crate::frames`] carries either
//! `#[serde(with = "serde_bytes")]` or, where the field may also arrive as
//! `nil`, `#[serde(default, with = "nullable_bytes")]`. `uuid::Uuid` already
//! encodes as a 16 byte `bin` under a non-human-readable serializer, matching
//! Go's UUID handling.
//!
//! ## Byte-exactness is not required
//!
//! Go writes integers at fixed width (`i64` always as `0xd3`) while `rmp-serde`
//! writes them compactly. Both are valid MessagePack and each decodes the
//! other. This never affects signatures, because a signature always covers the
//! exact payload bytes that were transmitted: on receipt we keep the original
//! buffer in `OriginalPayload` and verify against that rather than re-encoding.
//! The same applies to group IDs, which hash the transmitted creation blob.

use serde::{de::DeserializeOwned, Serialize};

use crate::error::Result;

/// Encode a value the way the Go implementation does: structs as maps keyed by
/// field name.
pub fn to_vec<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut serializer = rmp_serde::Serializer::new(&mut buf)
        .with_struct_map()
        .with_bytes(rmp_serde::config::BytesMode::ForceAll);
    value.serialize(&mut serializer)?;
    Ok(buf)
}

/// Decode a value encoded by either implementation.
pub fn from_slice<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    Ok(rmp_serde::from_slice(bytes)?)
}

/// Deserialize a sequence that may arrive as MessagePack `nil`.
///
/// Go distinguishes a nil slice from an empty one, and encodes the first as
/// `nil` (`0xc0`) rather than a zero-length array (`0x90`). A struct field left
/// unassigned — the attachment lists on a plain text message, say — therefore
/// arrives as `nil`, which a plain `Vec<T>` cannot decode.
///
/// Serialization is unaffected: we always emit a real array, which Go reads
/// back as an empty slice.
pub fn nullable_seq<'de, D, T>(deserializer: D) -> std::result::Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    use serde::Deserialize;
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
}

/// A `[]byte` field, in both directions.
///
/// Use it as `#[serde(default, with = "crate::msgpack::nullable_bytes")]`.
/// Both halves exist because both halves are wrong by default, in opposite
/// ways:
///
/// **Reading.** The same problem as [`nullable_seq`]: an unset Go `[]byte`
/// encodes as `nil` (`0xc0`), not as an empty `bin`, and a plain `Vec<u8>`
/// cannot decode that.
///
/// **Writing.** [`to_vec`] asks rmp-serde to encode byte sequences as `bin`,
/// but that decision is made by looking at the elements — and an *empty*
/// sequence has none to look at, so it comes out as an empty array (`0x90`).
/// Go then refuses it with "invalid code=90 decoding string/bytes length". An
/// empty byte field is not an edge case: an unencrypted file has no key and no
/// nonce, and an unsigned offer has no signature. Serialising through
/// `serde_bytes` calls `serialize_bytes` outright, so the length never enters
/// into it.
pub mod nullable_bytes {
    pub fn serialize<S>(value: &[u8], serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_bytes(value)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> std::result::Result<Vec<u8>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::Deserialize;

        /// Accepts either `nil` or a byte string, without going through
        /// `Option<Vec<u8>>` — which serde would decode as an array of
        /// integers rather than a `bin`.
        #[derive(Deserialize)]
        struct MaybeBytes(#[serde(with = "serde_bytes")] Vec<u8>);

        Ok(Option::<MaybeBytes>::deserialize(deserializer)?
            .map(|wrapped| wrapped.0)
            .unwrap_or_default())
    }
}

#[cfg(test)]
mod nullable_tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Item {
        #[serde(rename = "Name")]
        name: String,
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Frame {
        #[serde(rename = "Items", default, deserialize_with = "nullable_seq")]
        items: Vec<Item>,
        #[serde(rename = "Blob", default, with = "nullable_bytes")]
        blob: Vec<u8>,
    }

    #[test]
    fn a_nil_sequence_decodes_as_empty() {
        // Go's `msgpack.Marshal` on a struct with unassigned slice fields.
        // 0x82 map(2), "Items" -> 0xc0 nil, "Blob" -> 0xc0 nil.
        let encoded = hex::decode("82a54974656d73c0a4426c6f62c0").unwrap();
        let frame: Frame = from_slice(&encoded).unwrap();

        assert!(frame.items.is_empty());
        assert!(frame.blob.is_empty());
    }

    #[test]
    fn a_present_sequence_still_decodes() {
        let frame = Frame {
            items: vec![Item {
                name: "one".into(),
            }],
            blob: vec![1, 2, 3],
        };
        let round_tripped: Frame = from_slice(&to_vec(&frame).unwrap()).unwrap();
        assert_eq!(round_tripped, frame);
    }

    #[test]
    fn an_empty_sequence_still_decodes() {
        // 0x82 map(2), "Items" -> 0x90 empty array, "Blob" -> 0xc4 0x00 empty bin.
        let encoded = hex::decode("82a54974656d7390a4426c6f62c400").unwrap();
        let frame: Frame = from_slice(&encoded).unwrap();
        assert!(frame.items.is_empty());
        assert!(frame.blob.is_empty());
    }

    #[test]
    fn an_empty_sequence_is_written_as_an_array() {
        // Go reads an empty array back as an empty slice, so emitting `nil`
        // ourselves would gain nothing and lose clarity.
        let encoded = to_vec(&Frame {
            items: vec![],
            blob: vec![1],
        })
        .unwrap();
        assert!(encoded.contains(&0x90), "expected an empty-array header");
    }

    #[test]
    fn an_empty_byte_field_is_written_as_bin_not_as_an_array() {
        // rmp-serde decides between `bin` and an array by looking at a
        // sequence's elements, and an empty one has none — so without the
        // explicit `serialize_bytes` this comes out as `0x90` and Go rejects
        // the whole frame with "invalid code=90 decoding string/bytes length".
        // An unencrypted file has no key and no nonce, so this is the common
        // case, not a corner of one.
        let encoded = to_vec(&Frame {
            items: vec![Item { name: "one".into() }],
            blob: vec![],
        })
        .unwrap();

        // 0xc4 is bin8; 0x00 is its length.
        assert!(
            encoded.windows(2).any(|pair| pair == [0xc4, 0x00]),
            "expected an empty bin header, got {}",
            hex::encode(&encoded),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use uuid::Uuid;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct SignedContainerShape {
        #[serde(rename = "Signer")]
        signer: String,
        #[serde(rename = "Payload")]
        #[serde(with = "serde_bytes")]
        payload: Vec<u8>,
        #[serde(rename = "Signature")]
        #[serde(with = "serde_bytes")]
        signature: Vec<u8>,
    }

    /// Byte-for-byte output captured from the Go implementation:
    ///
    /// ```go
    /// sc := signedContainer{Signer: "abc.onion", Payload: []byte{1,2,3}, Signature: []byte{9,9}}
    /// msgpack.Marshal(sc)
    /// ```
    const GO_SIGNED_CONTAINER: &str =
        "83a65369676e6572a96162632e6f6e696f6ea75061796c6f6164c403010203a95369676e6174757265c4020909";

    #[test]
    fn matches_go_signed_container_bytes() {
        let value = SignedContainerShape {
            signer: "abc.onion".into(),
            payload: vec![1, 2, 3],
            signature: vec![9, 9],
        };
        assert_eq!(hex::encode(to_vec(&value).unwrap()), GO_SIGNED_CONTAINER);
    }

    #[test]
    fn decodes_go_signed_container_bytes() {
        let bytes = hex::decode(GO_SIGNED_CONTAINER).unwrap();
        let decoded: SignedContainerShape = from_slice(&bytes).unwrap();
        assert_eq!(decoded.signer, "abc.onion");
        assert_eq!(decoded.payload, vec![1, 2, 3]);
        assert_eq!(decoded.signature, vec![9, 9]);
    }

    #[test]
    fn uuid_encodes_as_sixteen_byte_bin() {
        // Captured from Go: msgpack.Marshal(uuid.MustParse(...))
        let id = Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap();
        assert_eq!(
            hex::encode(to_vec(&id).unwrap()),
            "c41011111111222233334444555555555555"
        );
        assert_eq!(
            hex::encode(to_vec(&Uuid::nil()).unwrap()),
            "c41000000000000000000000000000000000"
        );
    }

    #[test]
    fn uuid_round_trips() {
        let id = Uuid::new_v4();
        let encoded = to_vec(&id).unwrap();
        assert_eq!(from_slice::<Uuid>(&encoded).unwrap(), id);
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct WithOptional {
        #[serde(rename = "Signature")]
        signature: Option<u8>,
    }

    #[test]
    fn none_encodes_as_msgpack_nil() {
        // Go writes a nil pointer field as 0xc0; the map header plus the key
        // "Signature" precede it.
        let encoded = to_vec(&WithOptional { signature: None }).unwrap();
        assert_eq!(*encoded.last().unwrap(), 0xc0);
        assert_eq!(
            from_slice::<WithOptional>(&encoded).unwrap(),
            WithOptional { signature: None }
        );
    }
}
