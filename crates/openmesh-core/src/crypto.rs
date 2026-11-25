//! Identity, key agreement and payload sealing for OpenMesh.
//!
//! Two long-term key pairs per node:
//! * Ed25519 for signatures (announces, group messages, admin frames).
//! * X25519 for static Diffie-Hellman (direct messages, key hints, store-and-forward offers).
//!
//! Node ids are `SHA-256("openmesh/v1/node" || ed25519_pub || x25519_pub)[0..8]`, so they are
//! self-certifying: a node id can always be validated against the public keys.
//!
//! Direct messages use XChaCha20-Poly1305 with a key derived from the static DH shared secret
//! between the two identities. The 24-byte nonce is transmitted as a 12-byte random prefix
//! followed by twelve zero bytes. See `docs/architecture/PROTOCOL.md` §Cryptography.

use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use hkdf::Hkdf;
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey as X25519Public, StaticSecret};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::error::{Error, Result};
use crate::wire::NodeId;

/// Random bytes mixed into the node id hash.
const NODE_ID_DOMAIN: &[u8] = b"openmesh/v1/node";
/// HKDF salt for direct-message keys.
const DM_KDF_SALT: &[u8] = b"openmesh/v1/dm";
/// HKDF salt for channel keys.
const CHANNEL_KDF_SALT: &[u8] = b"openmesh/v1/channel";
/// Length of the transmitted random nonce prefix.
pub const NONCE_PREFIX_LEN: usize = 12;
/// AEAD tag length for XChaCha20-Poly1305.
pub const TAG_LEN: usize = 16;
/// Bytes added to a plaintext by [`Identity::seal`] / [`ChannelKey::seal`].
pub const SEAL_OVERHEAD: usize = NONCE_PREFIX_LEN + TAG_LEN;
/// Length of a serialized [`PublicIdentity`].
pub const PUBLIC_IDENTITY_LEN: usize = 8 + 32 + 32;

/// A node's public identity: address plus both verification keys.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PublicIdentity {
    /// Self-certifying node address.
    pub node_id: NodeId,
    /// Ed25519 verification key.
    pub sign_pk: [u8; 32],
    /// X25519 static Diffie-Hellman public key.
    pub dh_pk: [u8; 32],
}

impl PublicIdentity {
    /// Derive the address from the two public keys.
    pub fn from_keys(sign_pk: [u8; 32], dh_pk: [u8; 32]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(NODE_ID_DOMAIN);
        hasher.update(sign_pk);
        hasher.update(dh_pk);
        let digest: [u8; 32] = hasher.finalize().into();
        let mut node_id = NodeId::from_hash(&digest);
        // Address class rule (PROTOCOL.md §3): unicast node ids have the high bit cleared.
        node_id.0[0] &= 0x7F;
        Self {
            node_id,
            sign_pk,
            dh_pk,
        }
    }

    /// Verify that a claimed address matches the keys.
    pub fn is_self_consistent(&self) -> bool {
        Self::from_keys(self.sign_pk, self.dh_pk).node_id == self.node_id
    }

    /// Fixed 72-byte encoding (id || sign key || dh key).
    pub fn to_bytes(&self) -> [u8; PUBLIC_IDENTITY_LEN] {
        let mut out = [0u8; PUBLIC_IDENTITY_LEN];
        out[..8].copy_from_slice(&self.node_id.0);
        out[8..40].copy_from_slice(&self.sign_pk);
        out[40..72].copy_from_slice(&self.dh_pk);
        out
    }

    /// Parse the fixed 72-byte encoding (`node_id || sign_pk || dh_pk`).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != PUBLIC_IDENTITY_LEN {
            return Err(Error::Malformed("public identity must be 72 bytes"));
        }
        let mut id = [0u8; 8];
        id.copy_from_slice(&bytes[..8]);
        let mut sign_pk = [0u8; 32];
        sign_pk.copy_from_slice(&bytes[8..40]);
        let mut dh_pk = [0u8; 32];
        dh_pk.copy_from_slice(&bytes[40..72]);
        Ok(Self {
            node_id: NodeId(id),
            sign_pk,
            dh_pk,
        })
    }

    /// Verify a signature produced by the matching private identity.
    pub fn verify(&self, message: &[u8], signature: &[u8; 64]) -> Result<()> {
        let key = VerifyingKey::from_bytes(&self.sign_pk).map_err(|_| Error::Signature)?;
        let sig = Signature::from_bytes(signature);
        key.verify(message, &sig).map_err(|_| Error::Signature)
    }
}

/// A node's secret identity. Zeroized on drop.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct Identity {
    seed: [u8; 32],
    #[zeroize(skip)]
    signing: SigningKey,
    #[zeroize(skip)]
    dh: StaticSecret,
}

impl core::fmt::Debug for Identity {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Identity({})", self.public().node_id)
    }
}

impl Identity {
    /// Generate from the operating system CSPRNG.
    pub fn generate() -> Self {
        let mut seed = [0u8; 32];
        OsRng.fill_bytes(&mut seed);
        // A generated seed is already uniformly random; Ed25519's own clamping/derivation is
        // applied by `SigningKey::from_bytes`.
        Self::from_seed(&seed)
    }

    /// Deterministic construction from a 32-byte master secret.
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        let signing = SigningKey::from_bytes(seed);
        // Derive the X25519 static secret from the same seed but in a separate domain.
        let hk = Hkdf::<Sha256>::new(Some(b"openmesh/v1/dh"), seed);
        let mut dh_bytes = [0u8; 32];
        hk.expand(b"static-x25519", &mut dh_bytes)
            .expect("32 bytes is a valid HKDF-SHA256 output length");
        let dh = StaticSecret::from(dh_bytes);
        dh_bytes.zeroize();
        Self {
            seed: *seed,
            signing,
            dh,
        }
    }

    /// The 32-byte master secret (used for encrypted local storage).
    pub fn seed(&self) -> [u8; 32] {
        self.seed
    }

    /// Derive the public identity (keys are recomputed from the seed).
    pub fn public(&self) -> PublicIdentity {
        let sign_pk = self.signing.verifying_key().to_bytes();
        let dh_pk = X25519Public::from(&self.dh).to_bytes();
        PublicIdentity::from_keys(sign_pk, dh_pk)
    }

    /// This node's address.
    pub fn node_id(&self) -> NodeId {
        self.public().node_id
    }

    /// Sign a message with Ed25519.
    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        self.signing.sign(message).to_bytes()
    }

    /// Raw X25519 shared secret with a peer's DH key.
    fn dh_shared(&self, peer_dh_pk: &[u8; 32]) -> [u8; 32] {
        let peer = X25519Public::from(*peer_dh_pk);
        self.dh.diffie_hellman(&peer).to_bytes()
    }

    /// Derive the symmetric direct-message key for `peer`.
    ///
    /// `info` binds the key to both node ids in sorted order so both directions derive the
    /// same key while remaining distinct from any other pair.
    pub fn dm_key(&self, peer: &PublicIdentity) -> Result<[u8; 32]> {
        let me = self.public().node_id;
        let shared = self.dh_shared(&peer.dh_pk);
        let (lo, hi) = if me.0 <= peer.node_id.0 {
            (me, peer.node_id)
        } else {
            (peer.node_id, me)
        };
        let mut info = [0u8; 16];
        info[..8].copy_from_slice(&lo.0);
        info[8..].copy_from_slice(&hi.0);
        let hk = Hkdf::<Sha256>::new(Some(DM_KDF_SALT), &shared);
        let mut key = [0u8; 32];
        hk.expand(&info, &mut key)
            .map_err(|_| Error::Config("hkdf"))?;
        Ok(key)
    }

    /// Seal a plaintext for `peer`, returning `nonce_prefix || ciphertext || tag`.
    pub fn seal(&self, peer: &PublicIdentity, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>> {
        let key = self.dm_key(peer)?;
        let mut nonce = [0u8; 24];
        OsRng.fill_bytes(&mut nonce[..NONCE_PREFIX_LEN]);
        let out = seal_with_key(&key, &nonce, aad, plaintext)?;
        Ok(out)
    }

    /// Open a sealed payload from `peer`.
    pub fn open(&self, peer: &PublicIdentity, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>> {
        let key = self.dm_key(peer)?;
        open_with_key(&key, aad, sealed)
    }

    /// Seal a plaintext for `peer` with a caller-supplied nonce prefix.
    ///
    /// **Only for tests and golden vectors**: real traffic must use [`Identity::seal`], which
    /// draws the prefix from the OS CSPRNG. Reusing a nonce prefix with the same key and AAD
    /// catastrophically breaks XChaCha20-Poly1305.
    pub fn seal_with_nonce_prefix(
        &self,
        peer: &PublicIdentity,
        aad: &[u8],
        plaintext: &[u8],
        nonce_prefix: &[u8; NONCE_PREFIX_LEN],
    ) -> Result<Vec<u8>> {
        let key = self.dm_key(peer)?;
        let mut nonce = [0u8; 24];
        nonce[..NONCE_PREFIX_LEN].copy_from_slice(nonce_prefix);
        seal_with_key(&key, &nonce, aad, plaintext)
    }
}

/// A pre-shared channel (group) key.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct ChannelKey([u8; 32]);

impl core::fmt::Debug for ChannelKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("ChannelKey(<redacted>)")
    }
}

impl ChannelKey {
    /// Derive a channel key from a 32-byte pre-shared secret and a channel name.
    pub fn from_psk(psk: &[u8; 32], name: &str) -> Self {
        let hk = Hkdf::<Sha256>::new(Some(CHANNEL_KDF_SALT), psk);
        let mut key = [0u8; 32];
        let mut info = Vec::with_capacity(name.len() + 8);
        info.extend_from_slice(b"channel/");
        info.extend_from_slice(name.as_bytes());
        hk.expand(&info, &mut key)
            .expect("32 bytes is a valid HKDF output length");
        Self(key)
    }

    /// Channel address: `SHA-256("openmesh/v1/chan" || key)[0..8]` so the on-air destination
    /// does not reveal the channel name.
    pub fn id(&self) -> NodeId {
        let mut hasher = Sha256::new();
        hasher.update(b"openmesh/v1/chan");
        hasher.update(self.0);
        let digest: [u8; 32] = hasher.finalize().into();
        let mut id = NodeId::from_hash(&digest);
        // Address class rule (PROTOCOL.md §3): channel ids have the high bit set.
        id.0[0] |= 0x80;
        id
    }

    /// Seal a plaintext with the channel key, returning `nonce_prefix || ciphertext || tag`.
    pub fn seal(&self, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>> {
        let mut nonce = [0u8; 24];
        OsRng.fill_bytes(&mut nonce[..NONCE_PREFIX_LEN]);
        seal_with_key(&self.0, &nonce, aad, plaintext)
    }

    /// Open a payload sealed with the channel key.
    pub fn open(&self, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>> {
        open_with_key(&self.0, aad, sealed)
    }

    /// Seal a plaintext with a caller-supplied nonce prefix.
    ///
    /// **Only for tests and golden vectors**: real traffic must use [`ChannelKey::seal`],
    /// which draws the prefix from the OS CSPRNG.
    pub fn seal_with_nonce_prefix(
        &self,
        aad: &[u8],
        plaintext: &[u8],
        nonce_prefix: &[u8; NONCE_PREFIX_LEN],
    ) -> Result<Vec<u8>> {
        let mut nonce = [0u8; 24];
        nonce[..NONCE_PREFIX_LEN].copy_from_slice(nonce_prefix);
        seal_with_key(&self.0, &nonce, aad, plaintext)
    }
}

fn seal_with_key(
    key: &[u8; 32],
    nonce: &[u8; 24],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new(key.into());
    let ct = cipher
        .encrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| Error::Decrypt)?;
    let mut out = Vec::with_capacity(NONCE_PREFIX_LEN + ct.len());
    out.extend_from_slice(&nonce[..NONCE_PREFIX_LEN]);
    out.extend_from_slice(&ct);
    Ok(out)
}

fn open_with_key(key: &[u8; 32], aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>> {
    if sealed.len() < SEAL_OVERHEAD {
        return Err(Error::BufferTooSmall {
            need: SEAL_OVERHEAD,
            have: sealed.len(),
        });
    }
    let mut nonce = [0u8; 24];
    nonce[..NONCE_PREFIX_LEN].copy_from_slice(&sealed[..NONCE_PREFIX_LEN]);
    let cipher = XChaCha20Poly1305::new(key.into());
    cipher
        .decrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &sealed[NONCE_PREFIX_LEN..],
                aad,
            },
        )
        .map_err(|_| Error::Decrypt)
}

// ---------------------------------------------------------------------------------------------
// Serde: public identities are encoded as a compact 72-byte CBOR byte string.
// ---------------------------------------------------------------------------------------------

impl serde::Serialize for PublicIdentity {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> core::result::Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&self.to_bytes())
    }
}

impl<'de> serde::Deserialize<'de> for PublicIdentity {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> core::result::Result<Self, D::Error> {
        struct BytesVisitor;
        impl serde::de::Visitor<'_> for BytesVisitor {
            type Value = PublicIdentity;
            fn expecting(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.write_str("a 72-byte public identity")
            }
            fn visit_bytes<E: serde::de::Error>(
                self,
                v: &[u8],
            ) -> core::result::Result<Self::Value, E> {
                PublicIdentity::from_bytes(v).map_err(E::custom)
            }
            fn visit_byte_buf<E: serde::de::Error>(
                self,
                v: Vec<u8>,
            ) -> core::result::Result<Self::Value, E> {
                PublicIdentity::from_bytes(&v).map_err(E::custom)
            }
        }
        deserializer.deserialize_bytes(BytesVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_deterministic_and_self_certifying() {
        let a = Identity::from_seed(&[7u8; 32]);
        let b = Identity::from_seed(&[7u8; 32]);
        assert_eq!(a.node_id(), b.node_id());
        assert!(a.public().is_self_consistent());
        assert_ne!(a.node_id(), Identity::generate().node_id());
        assert_eq!(
            PublicIdentity::from_bytes(&a.public().to_bytes()).unwrap(),
            a.public()
        );
    }

    #[test]
    fn direct_message_roundtrip() {
        let alice = Identity::from_seed(&[1u8; 32]);
        let bob = Identity::from_seed(&[2u8; 32]);
        let aad = b"header";
        let sealed = alice.seal(&bob.public(), aad, b"hello bob").unwrap();
        assert_eq!(
            bob.open(&alice.public(), aad, &sealed).unwrap(),
            b"hello bob"
        );
        // Reverse direction derives the same key.
        let sealed2 = bob.seal(&alice.public(), aad, b"hello alice").unwrap();
        assert_eq!(
            alice.open(&bob.public(), aad, &sealed2).unwrap(),
            b"hello alice"
        );
    }

    #[test]
    fn tampered_ciphertext_is_rejected() {
        let alice = Identity::from_seed(&[1u8; 32]);
        let bob = Identity::from_seed(&[2u8; 32]);
        let mut sealed = alice.seal(&bob.public(), b"hdr", b"payload").unwrap();
        let last = sealed.len() - 1;
        sealed[last] ^= 0xFF;
        assert!(matches!(
            bob.open(&alice.public(), b"hdr", &sealed),
            Err(Error::Decrypt)
        ));
    }

    #[test]
    fn aad_is_authenticated() {
        let alice = Identity::from_seed(&[1u8; 32]);
        let bob = Identity::from_seed(&[2u8; 32]);
        let sealed = alice
            .seal(&bob.public(), b"header-one", b"payload")
            .unwrap();
        assert!(bob.open(&alice.public(), b"header-two", &sealed).is_err());
    }

    #[test]
    fn signatures_verify() {
        let alice = Identity::from_seed(&[3u8; 32]);
        let sig = alice.sign(b"announce");
        assert!(alice.public().verify(b"announce", &sig).is_ok());
        assert!(alice.public().verify(b"other", &sig).is_err());
    }

    #[test]
    fn channel_key_roundtrip_and_id() {
        let psk = [9u8; 32];
        let k1 = ChannelKey::from_psk(&psk, "family");
        let k2 = ChannelKey::from_psk(&psk, "family");
        let other = ChannelKey::from_psk(&psk, "friends");
        assert_eq!(k1.id(), k2.id());
        assert_ne!(k1.id(), other.id());
        let sealed = k1.seal(b"aad", b"group message").unwrap();
        assert_eq!(k2.open(b"aad", &sealed).unwrap(), b"group message");
        assert!(other.open(b"aad", &sealed).is_err());
    }
}
