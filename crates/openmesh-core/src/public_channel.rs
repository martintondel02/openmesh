//! The built-in, deliberately public "Public Mesh" channel.
//!
//! This channel is not private: its 32-byte PSK is published here and in the UI/API. Anyone with
//! an OpenMesh client can read and write it; sender signatures still verify *who* sent a message,
//! but there is **no confidentiality**. The UI/API must label it as PUBLIC and never present it as
//! private.
//!
//! ## Canonical address
//!
//! [`PROTOCOL.md` §3/§5.2](../docs/architecture/PROTOCOL.md) defines the on-air channel address as
//! `SHA-256("openmesh/v1/chan" || channel_key)[0..8]` (high bit set), where
//! `channel_key = HKDF-SHA256(psk, "openmesh/v1/channel", "channel/" || name)`. The canonical id for
//! this name+PSK is therefore [`PUBLIC_CHANNEL_ID_HEX`] (`a16d63142854cef4`); the older
//! `baac0f9144756da1` value is only a name-derived *invite identifier* and is **not** the wire
//! address. Do not use it as a frame destination.
//!
//! This is a protocol-compatible channel like any other: no wire change, no special frame types.

use crate::NodeId;

/// User-visible channel name. Also feeds the channel-key HKDF, so it is part of the protocol
/// address derivation and must never change.
pub const PUBLIC_CHANNEL_NAME: &str = "Public Mesh";

/// Published 32-byte channel PSK (deliberately public; no confidentiality by design).
pub const PUBLIC_CHANNEL_PSK: [u8; 32] = [
    0xfb, 0x0f, 0x8a, 0x98, 0xb9, 0x16, 0xb1, 0x19, 0xb1, 0xae, 0xd4, 0x2a, 0x70, 0xf4, 0xf6, 0xb1,
    0x7a, 0xe5, 0x71, 0x31, 0xd3, 0x89, 0xa0, 0x8d, 0xbb, 0x51, 0x7e, 0x42, 0x9f, 0xd8, 0xa9, 0x92,
];

/// Base64url (no padding) form of [`PUBLIC_CHANNEL_PSK`], used in invite URLs and API payloads.
pub const PUBLIC_CHANNEL_PSK_BASE64URL: &str = "-w-KmLkWsRmxrtQqcPT2sXrlcTHTiaCNu1F-Qp_YqZI";

/// Canonical protocol-derived 8-byte channel address (high bit set), as lowercase hex.
pub const PUBLIC_CHANNEL_ID_HEX: &str = "a16d63142854cef4";

/// Cosmetic name-derived identifier from the original task constants
/// (`SHA-256("openmesh/v1/public-channel/Public Mesh")[0..8]`). It is **not** the on-air channel
/// address; it may be shown as an invite/posting identifier only.
pub const PUBLIC_CHANNEL_INVITE_ID_HEX: &str = "baac0f9144756da1";

/// Canonical invite/join URL. It carries only name + published PSK; clients derive the channel id
/// from them (like the bridge does), so the URL deliberately has no `id` parameter.
pub const PUBLIC_CHANNEL_JOIN_URL: &str =
    "openmesh://channel?name=Public%20Mesh&psk=-w-KmLkWsRmxrtQqcPT2sXrlcTHTiaCNu1F-Qp_YqZI";

/// Full description of the built-in public channel (single source for every embedder).
#[derive(Clone, Copy, Debug)]
pub struct PublicChannel {
    /// User-visible name ([`PUBLIC_CHANNEL_NAME`]).
    pub name: &'static str,
    /// Canonical 32-byte PSK ([`PUBLIC_CHANNEL_PSK`]).
    pub psk: [u8; 32],
    /// Base64url form of [`Self::psk`].
    pub psk_base64url: &'static str,
    /// Canonical protocol-derived on-air/API channel address.
    pub id: NodeId,
    /// Lowercase hex form of [`Self::id`].
    pub id_hex: &'static str,
    /// Cosmetic name-derived invite identifier (not the wire address).
    pub invite_id_hex: &'static str,
    /// Canonical join URL.
    pub join_url: &'static str,
    /// Always `true`; marks the channel as public/no-confidentiality in API payloads.
    pub public: bool,
}

/// The built-in Public Mesh channel metadata.
pub const PUBLIC_CHANNEL: PublicChannel = PublicChannel {
    name: PUBLIC_CHANNEL_NAME,
    psk: PUBLIC_CHANNEL_PSK,
    psk_base64url: PUBLIC_CHANNEL_PSK_BASE64URL,
    id: NodeId::new([0xa1, 0x6d, 0x63, 0x14, 0x28, 0x54, 0xce, 0xf4]),
    id_hex: PUBLIC_CHANNEL_ID_HEX,
    invite_id_hex: PUBLIC_CHANNEL_INVITE_ID_HEX,
    join_url: PUBLIC_CHANNEL_JOIN_URL,
    public: true,
};

/// Return the built-in Public Mesh channel metadata.
pub fn public_channel() -> &'static PublicChannel {
    &PUBLIC_CHANNEL
}

/// The canonical Public Mesh channel address.
pub fn public_channel_id() -> NodeId {
    PUBLIC_CHANNEL.id
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ChannelKey;
    use base64::Engine as _;
    use sha2::{Digest, Sha256};

    #[test]
    fn canonical_id_is_derived_from_name_and_psk() {
        // This is the PROTOCOL.md §3/§5.2 derivation: channel_key = HKDF-SHA256(psk, salt,
        // "channel/"||name); channel_id = SHA-256("openmesh/v1/chan" || channel_key)[0..8], high bit
        // set. The task's original baac… value is only a cosmetic name hash, not the wire address.
        let derived = ChannelKey::from_psk(&PUBLIC_CHANNEL_PSK, PUBLIC_CHANNEL_NAME).id();
        assert_eq!(derived, PUBLIC_CHANNEL.id);
        assert_eq!(derived.to_hex(), PUBLIC_CHANNEL_ID_HEX);
        assert!(derived.is_channel());
        assert_ne!(PUBLIC_CHANNEL_ID_HEX, PUBLIC_CHANNEL_INVITE_ID_HEX);
    }

    #[test]
    fn invite_id_is_the_documented_name_hash() {
        let digest: [u8; 32] =
            Sha256::digest(format!("openmesh/v1/public-channel/{PUBLIC_CHANNEL_NAME}")).into();
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&digest[..8]);
        assert_eq!(hex::encode(bytes), PUBLIC_CHANNEL_INVITE_ID_HEX);
    }

    #[test]
    fn psk_and_join_url_are_consistent() {
        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(PUBLIC_CHANNEL_PSK_BASE64URL)
            .expect("base64url psk");
        assert_eq!(decoded, PUBLIC_CHANNEL_PSK);
        assert_eq!(
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(PUBLIC_CHANNEL_PSK),
            PUBLIC_CHANNEL_PSK_BASE64URL
        );
        let url = PUBLIC_CHANNEL_JOIN_URL;
        assert!(url.starts_with("openmesh://channel?"));
        assert!(url.contains("name=Public%20Mesh"));
        assert!(
            !url.contains("id="),
            "the join URL does not need the derived id"
        );
        assert!(url.contains(&format!("psk={PUBLIC_CHANNEL_PSK_BASE64URL}")));
        assert!(!url.contains(PUBLIC_CHANNEL_INVITE_ID_HEX));
        let public = PUBLIC_CHANNEL.public;
        assert!(public, "the built-in channel must be marked public");
    }
}
