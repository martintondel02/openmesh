//! Golden vector generation for the cross-language OMP/1 contract.
//!
//! `protocol/vectors/*.json` is normative test data shared by the Rust host stack, the C++
//! firmware and the mobile clients (PROTOCOL.md §15). This module is the single generator: the
//! `gen_vectors` example writes the files, and the `vectors_are_current` unit test regenerates
//! them in memory and fails if the committed files have drifted.
//!
//! Everything here is deterministic: fixed seeds, fixed nonces, pseudo-random payloads derived
//! from SHA-256, and sorted JSON objects. Regenerate with:
//!
//! ```text
//! cargo run -p openmesh-core --example gen_vectors
//! ```

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::crypto::{ChannelKey, Identity, PublicIdentity};
use crate::engine::{
    DEFAULT_TTL, PAYLOAD_IDENTITY_INCLUDED, PAYLOAD_IS_ACK, PAYLOAD_IS_MANIFEST, chunk_body,
    origin_ttl, sealed_body,
};
use crate::resource::{ack_bitmap, chunk_count, resource_aad};
use crate::services::{
    self, Ack, AckKind, Announce, AppMessage, AttachmentRef, MessageKind, NodeClass, Position,
    ResourceKind, ResourceManifest, Telemetry, announce_signature_payload, cbor_bytes,
};
use crate::wire::{BROADCAST, Flags, Frame, FrameType, HEADER_LEN, MAX_BODY, MTU, NodeId};

/// File names written into the vectors directory, in a stable order.
pub const VECTOR_FILES: &[&str] = &[
    "README.md",
    "frame.json",
    "identity.json",
    "seal.json",
    "announce.json",
    "app_message.json",
    "resource.json",
    "companion.json",
];

/// Default repository vectors directory (`<workspace>/protocol/vectors`).
pub fn default_vectors_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../protocol/vectors")
}

/// Generate every vector file into `out_dir` (created when missing).
///
/// Returns the list of files written, or an I/O error.
pub fn generate_all(out_dir: &Path) -> std::io::Result<Vec<String>> {
    fs::create_dir_all(out_dir)?;
    let mut written = Vec::new();
    for (name, contents) in generate_files() {
        fs::write(out_dir.join(name), contents)?;
        written.push(name.to_string());
    }
    Ok(written)
}

/// Build the contents of every vector file in memory as `(file_name, contents)` pairs.
pub fn generate_files() -> Vec<(&'static str, String)> {
    vec![
        ("README.md", readme()),
        ("frame.json", render(frame_vectors())),
        ("identity.json", render(identity_vectors())),
        ("seal.json", render(seal_vectors())),
        ("announce.json", render(announce_vectors())),
        ("app_message.json", render(app_message_vectors())),
        ("resource.json", render(resource_vectors())),
        ("companion.json", render(companion_vectors())),
    ]
}

fn render(value: Value) -> String {
    let mut out = serde_json::to_string_pretty(&value).expect("vector JSON is serializable");
    out.push('\n');
    out
}

// ---------------------------------------------------------------------------------------------
// Deterministic fixtures
// ---------------------------------------------------------------------------------------------

const ALICE_SEED: [u8; 32] = [0x11; 32];
const BOB_SEED: [u8; 32] = [0x12; 32];
const CAROL_SEED: [u8; 32] = [0x13; 32];
const CHANNEL_PSK: [u8; 32] = [0x21; 32];
const CHANNEL_NAME: &str = "family";

fn alice() -> Identity {
    Identity::from_seed(&ALICE_SEED)
}

fn bob() -> Identity {
    Identity::from_seed(&BOB_SEED)
}

fn hex(bytes: &[u8]) -> String {
    hex::encode(bytes)
}

/// Deterministic pseudo-random bytes: SHA-256("openmesh/v1/vector" || tag || counter) stream.
fn pseudo_bytes(len: usize, tag: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(len + 32);
    let mut counter = 0u32;
    while out.len() < len {
        let mut hasher = Sha256::new();
        hasher.update(b"openmesh/v1/vector");
        hasher.update([tag]);
        hasher.update(counter.to_be_bytes());
        let digest: [u8; 32] = hasher.finalize().into();
        out.extend_from_slice(&digest);
        counter += 1;
    }
    out.truncate(len);
    out
}

fn identity_json(ident: &PublicIdentity) -> Value {
    json!({
        "node_id_hex": hex(&ident.node_id.0),
        "sign_pk_hex": hex(&ident.sign_pk),
        "dh_pk_hex": hex(&ident.dh_pk),
        "public_identity_hex": hex(&ident.to_bytes()),
    })
}

fn announce_json(announce: &Announce) -> Value {
    json!({
        "version": announce.version,
        "class": announce.class.as_str(),
        "class_u8": announce.class.as_u8(),
        "caps": announce.caps,
        "name": announce.name,
        "hops_ui_only": announce.hops,
        "interval_s": announce.interval_s,
        "timestamp_ms": announce.timestamp_ms,
        "ident": identity_json(&announce.ident),
    })
}

fn type_name(typ: FrameType) -> &'static str {
    match typ {
        FrameType::Announce => "announce",
        FrameType::Data => "data",
        FrameType::Ack => "ack",
        FrameType::Resource => "resource",
        FrameType::RouteRequest => "route_request",
        FrameType::RouteResponse => "route_response",
        FrameType::Probe => "probe",
        FrameType::ProbeResponse => "probe_response",
        FrameType::StoreOffer => "store_offer",
        FrameType::StoreWant => "store_want",
        FrameType::Admin => "admin",
    }
}

fn frame_json(frame: &Frame) -> Value {
    let mut names = Vec::new();
    if frame.flags.contains(Flags::ENCRYPTED) {
        names.push("enc");
    }
    if frame.flags.contains(Flags::ACK_REQUESTED) {
        names.push("ack_req");
    }
    if frame.flags.contains(Flags::PRIORITY) {
        names.push("priority");
    }
    json!({
        "type": frame.typ as u8,
        "type_name": type_name(frame.typ),
        "flags": frame.flags.bits(),
        "flag_names": names,
        "ttl": frame.ttl,
        "src_hex": hex(&frame.src.0),
        "dst_hex": hex(&frame.dst.0),
        "seq": frame.seq,
        "body_len": frame.body.len(),
        "body_hex": hex(&frame.body),
        "frame_hex": hex(&frame.encode().expect("frame encodes")),
        "aad_hex": hex(&frame.aad()),
    })
}

fn sample_frame(
    typ: FrameType,
    flags: u8,
    ttl: u8,
    src: NodeId,
    dst: NodeId,
    seq: u32,
    body: Vec<u8>,
) -> Frame {
    Frame {
        typ,
        flags: Flags::from_bits(flags),
        ttl,
        src,
        dst,
        seq,
        body,
    }
}

// ---------------------------------------------------------------------------------------------
// frame.json
// ---------------------------------------------------------------------------------------------

fn frame_vectors() -> Value {
    let alice = alice().public();
    let bob = bob().public();
    let encode = vec![
        frame_json(&sample_frame(
            FrameType::Data,
            Flags::ENCRYPTED | Flags::ACK_REQUESTED,
            5,
            alice.node_id,
            bob.node_id,
            0xDEAD_BEEF,
            vec![0xAA, 0xBB, 0xCC, 0xDD, 0xEE],
        )),
        frame_json(&sample_frame(
            FrameType::Announce,
            0,
            8,
            alice.node_id,
            BROADCAST,
            1,
            pseudo_bytes(152, 1),
        )),
        frame_json(&sample_frame(
            FrameType::Resource,
            Flags::ENCRYPTED,
            3,
            alice.node_id,
            bob.node_id,
            42,
            pseudo_bytes(MAX_BODY, 2),
        )),
        frame_json(&sample_frame(
            FrameType::Ack,
            Flags::ENCRYPTED | Flags::PRIORITY,
            DEFAULT_TTL,
            bob.node_id,
            alice.node_id,
            7,
            vec![0x01, 0x02],
        )),
        // StoreOffer/StoreWant are clear-text CBOR (PROTOCOL.md §4.1, §10).
        frame_json(&sample_frame(
            FrameType::StoreOffer,
            Flags::PRIORITY,
            DEFAULT_TTL,
            bob.node_id,
            alice.node_id,
            0x0102_0304,
            cbor_bytes(&crate::services::StoreOffer {
                ids: vec![0x1122_3344, 0x5566_7788],
                count: 2,
            }),
        )),
    ];

    let decode_cases = [
        (
            "data_ttl3",
            sample_frame(
                FrameType::Data,
                Flags::ENCRYPTED,
                3,
                alice.node_id,
                bob.node_id,
                0x1122_3344,
                vec![9, 8, 7],
            ),
        ),
        (
            "announce_broadcast_empty_body",
            sample_frame(
                FrameType::Announce,
                0,
                8,
                alice.node_id,
                BROADCAST,
                2,
                vec![],
            ),
        ),
        (
            "ack_priority",
            sample_frame(
                FrameType::Ack,
                Flags::ENCRYPTED | Flags::PRIORITY,
                5,
                bob.node_id,
                alice.node_id,
                3,
                vec![0x42],
            ),
        ),
    ];
    let decode: Vec<Value> = decode_cases
        .into_iter()
        .map(|(name, frame)| {
            json!({
                "name": name,
                "frame_hex": hex(&frame.encode().expect("frame")),
                "expect": frame_json(&frame),
            })
        })
        .collect();

    // Malformed inputs: the Rust error kind is the expected `error` value.
    let valid = sample_frame(
        FrameType::Data,
        Flags::ENCRYPTED,
        5,
        alice.node_id,
        bob.node_id,
        1,
        vec![1],
    )
    .encode()
    .expect("frame");
    let mut bad_version = valid.clone();
    bad_version[0] = 2 << 5;
    let mut unknown_type = valid.clone();
    unknown_type[0] = (1 << 5) | 11;
    let mut oversize = valid.clone();
    oversize.resize(MTU + 1, 0);

    let decode_errors = vec![
        json!({"name": "empty", "frame_hex": "", "error": "BufferTooSmall"}),
        json!({"name": "short_header", "frame_hex": "0102030405", "error": "BufferTooSmall"}),
        json!({
            "name": "bad_version",
            "frame_hex": hex(&bad_version),
            "error": "UnsupportedVersion",
        }),
        json!({
            "name": "unknown_reserved_type",
            "frame_hex": hex(&unknown_type),
            "error": "Malformed",
        }),
        json!({"name": "oversize", "frame_hex": hex(&oversize), "error": "PayloadTooLarge"}),
    ];

    json!({
        "spec": "omp/1",
        "version": 1,
        "generated_by": "openmesh-core",
        "comment": "PROTOCOL.md §4: 22-byte header, big-endian, max 240 bytes. `aad_hex` is the header with the TTL nibble cleared. All byte strings are lowercase hex.",
        "mtu": MTU,
        "header_len": HEADER_LEN,
        "max_body": MAX_BODY,
        "broadcast_hex": hex(&BROADCAST.0),
        "encode": encode,
        "decode": decode,
        "decode_errors": decode_errors,
    })
}

// ---------------------------------------------------------------------------------------------
// identity.json
// ---------------------------------------------------------------------------------------------

fn identity_vectors() -> Value {
    let alice = alice();
    let bob = bob();
    let carol = Identity::from_seed(&CAROL_SEED);
    let message = b"openmesh/v1 test message";
    let signature = alice.sign(message);
    let mut tampered_message = message.to_vec();
    tampered_message[0] ^= 0x01;

    let dm_key = alice.dm_key(&bob.public()).expect("dm key");
    let channel = ChannelKey::from_psk(&CHANNEL_PSK, CHANNEL_NAME);

    let mut tampered_identity = alice.public().to_bytes();
    tampered_identity[0] ^= 0x01;

    json!({
        "spec": "omp/1",
        "version": 1,
        "generated_by": "openmesh-core",
        "comment": "PROTOCOL.md §3: seed -> Ed25519 + X25519 keys -> self-certifying 8-byte node id. Unicast node ids have bit 7 of byte 0 cleared; channel ids have it set.",
        "nodes": [
            {
                "name": "alice",
                "seed_hex": hex(&ALICE_SEED),
                "node_id_hex": hex(&alice.node_id().0),
                "sign_pk_hex": hex(&alice.public().sign_pk),
                "dh_pk_hex": hex(&alice.public().dh_pk),
                "public_identity_hex": hex(&alice.public().to_bytes()),
            },
            {
                "name": "bob",
                "seed_hex": hex(&BOB_SEED),
                "node_id_hex": hex(&bob.node_id().0),
                "public_identity_hex": hex(&bob.public().to_bytes()),
            },
            {
                "name": "carol",
                "seed_hex": hex(&CAROL_SEED),
                "node_id_hex": hex(&carol.node_id().0),
                "public_identity_hex": hex(&carol.public().to_bytes()),
            }
        ],
        "signatures": [{
            "name": "alice_signs_fixed_message",
            "seed_hex": hex(&ALICE_SEED),
            "message_hex": hex(message),
            "signature_hex": hex(&signature),
            "expect_verify": true,
            "tampered_message_hex": hex(&tampered_message),
            "expect_verify_tampered": false,
        }],
        "dm_keys": [{
            "name": "alice_bob",
            "seed_a_hex": hex(&ALICE_SEED),
            "seed_b_hex": hex(&BOB_SEED),
            "public_identity_a_hex": hex(&alice.public().to_bytes()),
            "public_identity_b_hex": hex(&bob.public().to_bytes()),
            "dm_key_hex": hex(&dm_key),
            "comment": "HKDF-SHA256(X25519(alice,bob), salt=openmesh/v1/dm, info=min(id)||max(id)); both directions derive the same key.",
        }],
        "channels": [{
            "name": CHANNEL_NAME,
            "psk_hex": hex(&CHANNEL_PSK),
            "channel_id_hex": hex(&channel.id().0),
            "class_bit_set": channel.id().is_channel(),
        }],
        "identity_errors": [
            json!({"name": "short_identity", "public_identity_hex": "00112233", "error": "Malformed"}),
            json!({"name": "long_identity", "public_identity_hex": "00".repeat(73), "error": "Malformed"}),
            json!({
                "name": "tampered_node_id_is_not_self_consistent",
                "public_identity_hex": hex(&tampered_identity),
                "self_consistent": false,
            }),
        ],
    })
}

// ---------------------------------------------------------------------------------------------
// seal.json
// ---------------------------------------------------------------------------------------------

const NONCE_PREFIX: [u8; 12] = [
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B,
];

fn seal_vectors() -> Value {
    let alice = alice();
    let bob = bob();
    let channel = ChannelKey::from_psk(&CHANNEL_PSK, CHANNEL_NAME);

    let dm_frame = sample_frame(
        FrameType::Data,
        Flags::ENCRYPTED | Flags::ACK_REQUESTED,
        5,
        alice.node_id(),
        bob.node_id(),
        7,
        vec![],
    );
    let dm_plaintext = b"hello mesh";
    let dm_sealed = alice
        .seal_with_nonce_prefix(&bob.public(), &dm_frame.aad(), dm_plaintext, &NONCE_PREFIX)
        .expect("seal");
    let dm_empty = alice
        .seal_with_nonce_prefix(&bob.public(), &dm_frame.aad(), b"", &NONCE_PREFIX)
        .expect("seal");

    let channel_id = channel.id();
    let channel_frame = sample_frame(
        FrameType::Data,
        Flags::ENCRYPTED,
        5,
        alice.node_id(),
        channel_id,
        8,
        vec![],
    );
    let channel_plaintext = b"group plaintext";
    let channel_sealed = channel
        .seal_with_nonce_prefix(&channel_frame.aad(), channel_plaintext, &NONCE_PREFIX)
        .expect("seal");

    let identity_body = sealed_body(
        PAYLOAD_IDENTITY_INCLUDED,
        0x1122_3344,
        Some(&alice.public()),
        &dm_sealed,
    );
    let plain_body = sealed_body(0, 0x1122_3344, None, &dm_sealed);
    let ack_body = sealed_body(PAYLOAD_IS_ACK, 0xDEAD_BEEF, None, &dm_sealed);
    let manifest_body = sealed_body(PAYLOAD_IS_MANIFEST, 0x0102_0304, None, &dm_sealed);

    let mut tampered = dm_sealed.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 0xFF;

    json!({
        "spec": "omp/1",
        "version": 1,
        "generated_by": "openmesh-core",
        "comment": "PROTOCOL.md §5. nonce = nonce_prefix(12) || 0^12; AAD = 22-byte header with the TTL nibble cleared. Sealed payload = nonce_prefix || ct || tag. Fixed nonce prefixes are TEST-ONLY.",
        "dm": [{
            "name": "alice_to_bob_hello",
            "seed_a_hex": hex(&ALICE_SEED),
            "seed_b_hex": hex(&BOB_SEED),
            "public_identity_a_hex": hex(&alice.public().to_bytes()),
            "public_identity_b_hex": hex(&bob.public().to_bytes()),
            "frame_header": {
                "type": 1,
                "flags": Flags::ENCRYPTED | Flags::ACK_REQUESTED,
                "ttl": 5,
                "src_hex": hex(&alice.node_id().0),
                "dst_hex": hex(&bob.node_id().0),
                "seq": 7,
            },
            "aad_hex": hex(&dm_frame.aad()),
            "plaintext_hex": hex(dm_plaintext),
            "nonce_prefix_hex": hex(&NONCE_PREFIX),
            "sealed_hex": hex(&dm_sealed),
            "expect_open_ok": true,
            "note": "aad_hex[1] has the TTL nibble cleared: flags byte only, here 0x03.",
        }],
        "dm_empty_plaintext": [{
            "name": "empty_payload",
            "aad_hex": hex(&dm_frame.aad()),
            "plaintext_hex": "",
            "nonce_prefix_hex": hex(&NONCE_PREFIX),
            "sealed_hex": hex(&dm_empty),
            "expect_open_ok": true,
        }],
        "channel": [{
            "name": "family_group_plaintext",
            "channel_name": CHANNEL_NAME,
            "psk_hex": hex(&CHANNEL_PSK),
            "channel_id_hex": hex(&channel_id.0),
            "aad_hex": hex(&channel_frame.aad()),
            "plaintext_hex": hex(channel_plaintext),
            "nonce_prefix_hex": hex(&NONCE_PREFIX),
            "sealed_hex": hex(&channel_sealed),
            "expect_open_ok": true,
        }],
        "body_layouts": [
            {
                "name": "data_with_identity_hint",
                "payload_flags": PAYLOAD_IDENTITY_INCLUDED,
                "msg_ref": 0x1122_3344,
                "identity_hex": hex(&alice.public().to_bytes()),
                "sealed_hex": hex(&dm_sealed),
                "body_hex": hex(&identity_body),
            },
            {
                "name": "data_without_identity",
                "payload_flags": 0,
                "msg_ref": 0x1122_3344,
                "sealed_hex": hex(&dm_sealed),
                "body_hex": hex(&plain_body),
            },
            {
                "name": "ack_body",
                "payload_flags": PAYLOAD_IS_ACK,
                "msg_ref": 0xDEAD_BEEFu32,
                "sealed_hex": hex(&dm_sealed),
                "body_hex": hex(&ack_body),
            },
            {
                "name": "manifest_body",
                "payload_flags": PAYLOAD_IS_MANIFEST,
                "msg_ref": 0x0102_0304,
                "sealed_hex": hex(&dm_sealed),
                "body_hex": hex(&manifest_body),
            },
        ],
        "negative": [{
            "name": "tampered_ciphertext",
            "aad_hex": hex(&dm_frame.aad()),
            "sealed_hex": hex(&tampered),
            "expect_open_error": "Decrypt",
        }],
    })
}

// ---------------------------------------------------------------------------------------------
// announce.json
// ---------------------------------------------------------------------------------------------

fn announce_vectors() -> Value {
    let cases = [
        (
            "alice_client",
            Identity::from_seed(&ALICE_SEED),
            Announce {
                version: 1,
                class: NodeClass::Client,
                caps: services::caps::RELAY | services::caps::GPS,
                name: "alice".into(),
                hops: 0,
                timestamp_ms: 1_700_000_000_000,
                interval_s: 1_800,
                ident: Identity::from_seed(&ALICE_SEED).public(),
            },
            42u32,
        ),
        (
            "solar_ridge_repeater",
            Identity::from_seed(&BOB_SEED),
            Announce {
                version: 1,
                class: NodeClass::Repeater,
                caps: services::caps::RELAY | services::caps::STORE | services::caps::SOLAR,
                name: "solar-ridge".into(),
                hops: 0,
                timestamp_ms: 1_700_000_123_456,
                interval_s: 3_600,
                ident: Identity::from_seed(&BOB_SEED).public(),
            },
            43u32,
        ),
        (
            "empty_name_tracker",
            Identity::from_seed(&CAROL_SEED),
            Announce {
                version: 1,
                class: NodeClass::Tracker,
                caps: services::caps::GPS,
                name: String::new(),
                hops: 0,
                timestamp_ms: 1,
                interval_s: 1_800,
                ident: Identity::from_seed(&CAROL_SEED).public(),
            },
            1u32,
        ),
    ];

    let mut encoded = Vec::new();
    let mut rejects = Vec::new();
    for (name, signer, announce, seq) in cases {
        let unsigned = announce.encode_wire().expect("announce encodes");
        let payload = announce_signature_payload(&announce.ident.node_id, seq, &unsigned);
        let signature = signer.sign(&payload);
        let body = announce.encode_signed(&signature).expect("signed announce");
        let frame_ttl8 = sample_frame(
            FrameType::Announce,
            0,
            8,
            announce.ident.node_id,
            BROADCAST,
            seq,
            body.clone(),
        );
        let mut relayed = frame_ttl8.clone();
        relayed.ttl = 7;
        encoded.push(json!({
            "name": name,
            "src_hex": hex(&announce.ident.node_id.0),
            "seq": seq,
            "announce": announce_json(&announce),
            "body_without_signature_hex": hex(&unsigned),
            "body_without_signature_len": unsigned.len(),
            "signature_hex": hex(&signature),
            "body_hex": hex(&body),
            "body_len": body.len(),
            "frame_ttl8_hex": hex(&frame_ttl8.encode().expect("frame")),
            "relayed_ttl7": {
                "frame_hex": hex(&relayed.encode().expect("frame")),
                "hops_derived": 1,
                "note": "hop count = ANNOUNCE_TTL(8) - frame.ttl; relays never modify the signed body.",
            },
        }));

        if name == "alice_client" {
            let mut bad_signature = signature;
            bad_signature[0] ^= 0x01;
            let mut tampered_body = unsigned.clone();
            tampered_body.extend_from_slice(&bad_signature);
            rejects.push(json!({
                "name": "tampered_signature",
                "body_hex": hex(&tampered_body),
                "src_hex": hex(&announce.ident.node_id.0),
                "seq": seq,
                "error": "Signature",
            }));
            let mut reserved = unsigned.clone();
            reserved[4] = 1;
            reserved.extend_from_slice(&signature);
            rejects.push(json!({
                "name": "reserved_byte_nonzero",
                "body_hex": hex(&reserved),
                "src_hex": hex(&announce.ident.node_id.0),
                "seq": seq,
                "error": "Malformed",
            }));
            rejects.push(json!({
                "name": "truncated",
                "body_hex": hex(&body[..40]),
                "src_hex": hex(&announce.ident.node_id.0),
                "seq": seq,
                "error": "BufferTooSmall",
            }));
            let mut wrong_version = unsigned.clone();
            wrong_version[0] = 2;
            wrong_version.extend_from_slice(&signature);
            rejects.push(json!({
                "name": "wrong_version",
                "body_hex": hex(&wrong_version),
                "src_hex": hex(&announce.ident.node_id.0),
                "seq": seq,
                "error": "UnsupportedVersion",
            }));
        }
    }

    json!({
        "spec": "omp/1",
        "version": 1,
        "generated_by": "openmesh-core",
        "comment": "PROTOCOL.md §6 binary announce. signature = Ed25519(sk, \"openmesh/v1/announce\" || src(8) || seq(4 BE) || body_without_signature). `hops` is NOT on the wire (offset 4 is reserved zero); receivers derive hops = 8 - frame.ttl.",
        "prefix_len": services::ANNOUNCE_PREFIX_LEN,
        "signature_len": services::ANNOUNCE_SIG_LEN,
        "name_max": services::ANNOUNCE_NAME_MAX,
        "max_body_len": services::ANNOUNCE_MAX_BODY,
        "cases": encoded,
        "reject_cases": rejects,
    })
}

// ---------------------------------------------------------------------------------------------
// app_message.json
// ---------------------------------------------------------------------------------------------

fn app_message_vectors() -> Value {
    let attachment_ref = AttachmentRef {
        res_id: 0x0102_0304,
        mime: "image/jpeg".into(),
        size: 5_120,
        sha256: Sha256::digest(pseudo_bytes(5_120, 9)).into(),
        chunk_bytes: 180,
        chunk_count: 29,
        duration_ms: None,
        name: Some("trail.jpg".into()),
    };
    let text = AppMessage {
        id: 42,
        kind: MessageKind::Text,
        sent_at_ms: 1_700_000_000_000,
        text: Some("hello mesh".into()),
        reply_to: Some(41),
        thread: None,
        attachment: None,
        position: None,
        telemetry: None,
        sender: None,
        signature: None,
    };
    let position = AppMessage {
        id: 7,
        kind: MessageKind::Position,
        sent_at_ms: 1_700_000_000_500,
        text: None,
        reply_to: None,
        thread: None,
        attachment: None,
        position: Some(Position {
            lat_e7: 59_913_700,
            lon_e7: 10_752_200,
            alt_m: Some(45),
            accuracy_m: Some(8),
            speed_cm_s: Some(120),
            heading_deg: Some(180),
            timestamp_ms: 1_700_000_000_000,
        }),
        telemetry: None,
        sender: None,
        signature: None,
    };
    let telemetry = AppMessage {
        id: 8,
        kind: MessageKind::Telemetry,
        sent_at_ms: 1_700_000_001_000,
        text: None,
        reply_to: None,
        thread: None,
        attachment: None,
        position: None,
        telemetry: Some(Telemetry {
            timestamp_ms: 1_700_000_001_000,
            battery_mv: Some(3_980),
            battery_pct: Some(82),
            uptime_s: Some(86_400),
            airtime_used_ms: Some(42_000),
            rx_frames: Some(240),
            tx_frames: Some(120),
            temperature_c: Some(21),
            solar_mv: Some(5_100),
        }),
        sender: None,
        signature: None,
    };
    let attachment = AppMessage {
        id: 9,
        kind: MessageKind::Attachment,
        sent_at_ms: 1_700_000_002_000,
        text: None,
        reply_to: None,
        thread: None,
        attachment: Some(attachment_ref.clone()),
        position: None,
        telemetry: None,
        sender: None,
        signature: None,
    };

    let cases = vec![
        (
            "text_with_reply",
            text,
            json!({
                "id": 42,
                "kind": "text",
                "sent_at_ms": 1_700_000_000_000u64,
                "text": "hello mesh",
                "reply_to": 41,
            }),
        ),
        (
            "position",
            position,
            json!({
                "id": 7,
                "kind": "position",
                "sent_at_ms": 1_700_000_000_500u64,
                "position": {
                    "lat_e7": 59_913_700,
                    "lon_e7": 10_752_200,
                    "alt_m": 45,
                    "accuracy_m": 8,
                    "speed_cm_s": 120,
                    "heading_deg": 180,
                    "timestamp_ms": 1_700_000_000_000u64,
                },
            }),
        ),
        (
            "telemetry",
            telemetry,
            json!({
                "id": 8,
                "kind": "telemetry",
                "sent_at_ms": 1_700_000_001_000u64,
                "telemetry": {
                    "timestamp_ms": 1_700_000_001_000u64,
                    "battery_mv": 3980,
                    "battery_pct": 82,
                    "uptime_s": 86400,
                    "airtime_used_ms": 42000,
                    "rx_frames": 240,
                    "tx_frames": 120,
                    "temperature_c": 21,
                    "solar_mv": 5100,
                },
            }),
        ),
        (
            "attachment_ref",
            attachment,
            json!({
                "id": 9,
                "kind": "attachment",
                "sent_at_ms": 1_700_000_002_000u64,
                "attachment": {
                    "res_id": 0x0102_0304,
                    "mime": "image/jpeg",
                    "size": 5120,
                    "sha256_hex": hex(&attachment_ref.sha256),
                    "chunk_bytes": 180,
                    "chunk_count": 29,
                    "name": "trail.jpg",
                },
            }),
        ),
    ];

    let encoded: Vec<Value> = cases
        .into_iter()
        .map(|(name, message, value)| {
            json!({
                "name": name,
                "message": value,
                "cbor_hex": hex(&cbor_bytes(&message)),
            })
        })
        .collect();

    json!({
        "spec": "omp/1",
        "version": 1,
        "generated_by": "openmesh-core",
        "comment": "PROTOCOL.md §5.3/§7: CBOR maps with definite lengths; field names are the CBOR map keys. Byte fields are CBOR byte strings. Optional fields are omitted when absent.",
        "cases": encoded,
        "decode_errors": [
            json!({"name": "truncated", "cbor_hex": "a2", "error": "Codec"}),
            json!({"name": "missing_required_fields", "cbor_hex": "a1626964182a", "error": "Codec"}),
            json!({"name": "not_a_map", "cbor_hex": "83010203", "error": "Codec"}),
        ],
        "ack": [
            {
                "name": "delivered",
                "ack": {"kind": "delivered", "msg_id": 42, "timestamp_ms": 1_700_000_100_000u64},
                "cbor_hex": hex(&cbor_bytes(&Ack {
                    kind: AckKind::Delivered,
                    msg_id: 42,
                    res_id: None,
                    next_expected: None,
                    bitmap: None,
                    timestamp_ms: 1_700_000_100_000,
                })),
            },
            {
                "name": "resource_window",
                "ack": {
                    "kind": "resource_window",
                    "msg_id": 42,
                    "res_id": 0x0102_0304,
                    "next_expected": 3,
                    "bitmap": 0b1111_1000u32,
                    "timestamp_ms": 1_700_000_100_000u64,
                },
                "cbor_hex": hex(&cbor_bytes(&Ack {
                    kind: AckKind::ResourceWindow,
                    msg_id: 42,
                    res_id: Some(0x0102_0304),
                    next_expected: Some(3),
                    bitmap: Some(0b1111_1000),
                    timestamp_ms: 1_700_000_100_000,
                })),
                "bitmap_note": "bit 0 = chunk next_expected+1 = chunk 4; bits 3..7 set means chunks 7,8,9,10,11 received.",
            },
        ],
    })
}

// ---------------------------------------------------------------------------------------------
// resource.json
// ---------------------------------------------------------------------------------------------

fn resource_vectors() -> Value {
    let alice = alice();
    let bob = bob();
    let channel = ChannelKey::from_psk(&CHANNEL_PSK, CHANNEL_NAME);
    let res_id: u32 = 0x0102_0304;
    let msg_id: u32 = 0x1122_3344;
    let chunk_bytes: u16 = 180;
    let plaintext = pseudo_bytes(600, 7);
    let total_len = (1 + crate::crypto::SEAL_OVERHEAD + plaintext.len()) as u32;
    let count = chunk_count(total_len, chunk_bytes).expect("chunk count");
    let aad = resource_aad(res_id, msg_id, total_len, chunk_bytes);
    let sealed = alice
        .seal_with_nonce_prefix(&bob.public(), &aad, &plaintext, &NONCE_PREFIX)
        .expect("seal blob");
    let mut blob = Vec::with_capacity(1 + sealed.len());
    blob.push(0);
    blob.extend_from_slice(&sealed);
    let digest: [u8; 32] = Sha256::digest(&blob).into();
    let manifest = ResourceManifest {
        res_id,
        msg_id,
        kind: ResourceKind::Attachment,
        mime: "application/octet-stream".into(),
        total_len,
        chunk_bytes,
        chunk_count: count,
        sha256: digest,
    };
    let chunks: Vec<Value> = blob
        .chunks(chunk_bytes as usize)
        .enumerate()
        .map(|(index, slice)| {
            json!({
                "index": index as u16,
                "slice_hex": hex(slice),
                "slice_len": slice.len(),
                "body_hex": hex(&chunk_body(msg_id, res_id, index as u16, slice)),
            })
        })
        .collect();

    // Selective-repeat ack bitmaps: bit 0 reports chunk next_expected + 1.
    let bitmap_cases: Vec<(&str, u16, Vec<u16>)> = vec![
        ("empty", 0, Vec::new()),
        ("first_received", 0, vec![0]),
        ("hole_at_3", 3, vec![0, 1, 2, 4, 5, 6]),
        ("full_window", 0, (1..=31u16).collect()),
        ("next_window", 8, (8..=20u16).collect()),
    ];
    let ack_bitmaps: Vec<Value> = bitmap_cases
        .into_iter()
        .map(|(name, next_expected, received)| {
            let total = 32u16;
            let bitmap = ack_bitmap(total, next_expected, |i| received.contains(&i));
            json!({
                "name": name,
                "total_chunks": total,
                "next_expected": next_expected,
                "received": received,
                "bitmap": bitmap,
                "bitmap_bits_set": (0..32u16)
                    .filter(|bit| bitmap & (1 << bit) != 0)
                    .collect::<Vec<u16>>(),
            })
        })
        .collect();

    json!({
        "spec": "omp/1",
        "version": 1,
        "generated_by": "openmesh-core",
        "comment": "PROTOCOL.md §9. sealed_blob = payload_flags(1)=0 || nonce_prefix(12) || ct || tag; AAD = \"openmesh/v1/resource\" || res_id(4 BE) || msg_id(4 BE) || total_len(4 BE) || chunk_bytes(2 BE). sha256 is over the whole sealed blob (total_len bytes).",
        "chunk_bytes": chunk_bytes,
        "blob": {
            "name": "attachment_600_bytes",
            "res_id": res_id,
            "msg_id": msg_id,
            "plaintext_hex": hex(&plaintext),
            "plaintext_len": plaintext.len(),
            "nonce_prefix_hex": hex(&NONCE_PREFIX),
            "aad_hex": hex(&aad),
            "aad_len": aad.len(),
            "sealed_hex": hex(&sealed),
            "sealed_blob_hex": hex(&blob),
            "total_len": total_len,
            "sha256_hex": hex(&digest),
            "chunk_count": count,
        },
        "manifest": {
            "json": {
                "res_id": res_id,
                "msg_id": msg_id,
                "kind": "attachment",
                "mime": manifest.mime,
                "total_len": total_len,
                "chunk_bytes": chunk_bytes,
                "chunk_count": count,
                "sha256_hex": hex(&manifest.sha256),
            },
            "cbor_hex": hex(&cbor_bytes(&manifest)),
        },
        "chunks": chunks,
        "ack_bitmaps": ack_bitmaps,
        "channel_blob": {
            "name": "family_channel_attachment",
            "channel_name": CHANNEL_NAME,
            "psk_hex": hex(&CHANNEL_PSK),
            "channel_id_hex": hex(&channel.id().0),
            "aad_hex": hex(&resource_aad(0xBEEF, 0x0102, total_len, chunk_bytes)),
            "comment": "Channel resources use the channel key with the same 34-byte AAD layout.",
        },
        "hop_note": {
            "announce_origin_ttl": origin_ttl(FrameType::Announce),
            "default_ttl": origin_ttl(FrameType::Data),
        },
    })
}

// ---------------------------------------------------------------------------------------------
// companion.json
// ---------------------------------------------------------------------------------------------

fn companion_vectors() -> Value {
    use crate::companion::{
        self, COMPANION_CRC_LEN, COMPANION_HEADER_LEN, CompanionMsg, encode, encode_slip,
    };

    let node_id = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
    let messages: Vec<(&str, Value, CompanionMsg)> = vec![
        (
            "hello",
            json!({"proto": 1, "app_version": "openmesh-android/0.1.0", "wants_events": true}),
            CompanionMsg::Hello {
                proto: 1,
                app_version: "openmesh-android/0.1.0".into(),
                wants_events: true,
            },
        ),
        (
            "hello_ack",
            json!({
                "proto": 1,
                "fw_version": "0.1.0",
                "node_id_hex": hex(&node_id),
                "max_frame": 240,
                "caps": 9,
                "region": "EU868",
                "profile": "MEDIUM_FAST",
            }),
            CompanionMsg::HelloAck {
                proto: 1,
                fw_version: "0.1.0".into(),
                node_id,
                max_frame: 240,
                caps: 9,
                region: "EU868".into(),
                profile: "MEDIUM_FAST".into(),
            },
        ),
        ("config_get", json!({}), CompanionMsg::ConfigGet),
        (
            "config_set",
            json!({"key": "node.name", "value": "solar-ridge"}),
            CompanionMsg::ConfigSet {
                key: "node.name".into(),
                value: "solar-ridge".into(),
            },
        ),
        (
            "config",
            json!({"entries": [["node.name", "solar-ridge"], ["mesh.ttl", "5"]]}),
            CompanionMsg::Config {
                entries: vec![
                    ("node.name".into(), "solar-ridge".into()),
                    ("mesh.ttl".into(), "5".into()),
                ],
            },
        ),
        (
            "tx",
            json!({"frame_hex": hex(&pseudo_bytes(22, 3))}),
            CompanionMsg::Tx {
                frame: pseudo_bytes(22, 3),
            },
        ),
        (
            "rx",
            json!({
                "frame_hex": hex(&pseudo_bytes(22, 4)),
                "rssi": -101,
                "snr": 26,
                "link": "lora",
                "at_ms": 1_760_000_000_000u64,
            }),
            CompanionMsg::Rx {
                frame: pseudo_bytes(22, 4),
                rssi: -101,
                snr: 26,
                link: "lora".into(),
                at_ms: 1_760_000_000_000,
            },
        ),
        ("stats_get", json!({}), CompanionMsg::StatsGet),
        (
            "stats",
            json!({"entries": [["tx_frames", 120], ["rx_frames", 240], ["forwarded", 31]]}),
            CompanionMsg::Stats {
                entries: vec![
                    ("tx_frames".into(), 120),
                    ("rx_frames".into(), 240),
                    ("forwarded".into(), 31),
                ],
            },
        ),
        (
            "ping",
            json!({"nonce": 0xDEAD_BEEFu64}),
            CompanionMsg::Ping { nonce: 0xDEAD_BEEF },
        ),
        (
            "pong",
            json!({"nonce": 0xDEAD_BEEFu64}),
            CompanionMsg::Pong { nonce: 0xDEAD_BEEF },
        ),
        (
            "event",
            json!({"kind": "store_deliver", "detail": "replayed 2 envelopes"}),
            CompanionMsg::Event {
                kind: "store_deliver".into(),
                detail: "replayed 2 envelopes".into(),
            },
        ),
        ("reset", json!({}), CompanionMsg::Reset),
    ];

    let cases: Vec<Value> = messages
        .into_iter()
        .map(|(name, payload, message)| {
            let chunk = encode(&message).expect("companion encode");
            json!({
                "name": name,
                "type": message.message_type().as_u8(),
                "payload": payload,
                "payload_cbor_hex": hex(&chunk[COMPANION_HEADER_LEN..chunk.len() - COMPANION_CRC_LEN]),
                "frame_hex": hex(&chunk),
                "slip_hex": hex(&encode_slip(&message).expect("slip encode")),
            })
        })
        .collect();

    let mut bad_crc = encode(&CompanionMsg::Ping { nonce: 1 }).expect("encode");
    let last = bad_crc.len() - 1;
    bad_crc[last] ^= 0xFF;
    let mut bad_magic = encode(&CompanionMsg::Reset).expect("encode");
    bad_magic[0] = b'X';
    let mut unknown_type = encode(&CompanionMsg::Reset).expect("encode");
    unknown_type[2] = 99;
    let mut oversize = encode(&CompanionMsg::Reset).expect("encode");
    oversize[3..5].copy_from_slice(&(companion::COMPANION_MAX_PAYLOAD as u16 + 1).to_le_bytes());
    let partial = encode(&CompanionMsg::Ping { nonce: 1 }).expect("encode");

    json!({
        "spec": "omp/1",
        "version": 1,
        "generated_by": "openmesh-core",
        "comment": "docs/architecture/COMPANION-API.md §1: chunk = \"OM\" | type(u8) | len(u16 LE) | CBOR payload | crc16(u16 LE). CRC-16/CCITT-FALSE (poly 0x1021, init 0xFFFF, xorout 0) over type||len||payload. SLIP: 0xC0 delimiters, 0xC0 -> 0xDB 0xDC, 0xDB -> 0xDB 0xDD.",
        "magic_hex": hex(&companion::COMPANION_MAGIC),
        "header_len": COMPANION_HEADER_LEN,
        "crc_len": COMPANION_CRC_LEN,
        "max_payload": companion::COMPANION_MAX_PAYLOAD,
        "crc16_ccitt_false": [
            {"input_ascii": "123456789", "crc16": companion::crc16_ccitt_false(b"123456789")},
            {"input_hex": "", "crc16": companion::crc16_ccitt_false(b"")},
        ],
        "cases": cases,
        "error_cases": [
            {"name": "short_header", "frame_hex": "4f4d", "error": "BufferTooSmall"},
            {"name": "bad_magic", "frame_hex": hex(&bad_magic), "error": "Malformed"},
            {"name": "unknown_type", "frame_hex": hex(&unknown_type), "error": "Malformed"},
            {"name": "bad_crc", "frame_hex": hex(&bad_crc), "error": "Malformed"},
            {"name": "payload_too_large", "frame_hex": hex(&oversize), "error": "PayloadTooLarge"},
            {"name": "partial_frame", "frame_hex": hex(&partial[..6]), "error": "BufferTooSmall"},
        ],
    })
}

// ---------------------------------------------------------------------------------------------
// README.md
// ---------------------------------------------------------------------------------------------

fn readme() -> String {
    r#"# OMP/1 golden vectors

**Status:** normative test data for OpenMesh v0.1.0 (PROTOCOL.md §15).
Generated by `cargo run -p openmesh-core --example gen_vectors`; the unit test
`openmesh_core::vectors::tests::vectors_are_current` fails CI if these files drift from the
Rust reference implementation. Do not edit by hand. A change that alters a vector requires a
spec change and a protocol version bump.

Implementations that must pass these vectors: the Rust host stack (`crates/openmesh-core`),
the C++ firmware (`firmware/test/test_protocol/`) and the companion clients.

## Conventions

* Every file is a JSON object with `"spec": "omp/1"`, `"version": 1` and `"generated_by"`.
* All binary values are **lowercase hex strings without `0x` or separators** (`""` for empty).
  Fields ending in `_hex` are bytes; plain numbers are the value itself.
* Wire integers are **big-endian** except the companion `len` field, which is little-endian
  (noted where it appears).
* Positive cases carry everything needed to reproduce the output. Negative cases carry
  `"error"`: the Rust error kind the reference implementation returns. Error kind names:
  `BufferTooSmall`, `Malformed`, `UnsupportedVersion`, `Decrypt`, `Signature`,
  `PayloadTooLarge`, `Codec`, `Config`, `UnknownNode`, `Resource`.
* There is **no `"valid"` boolean**: positive cases live in arrays such as `encode`, `decode`,
  `cases`, `dm`, `body_layouts`, `chunks`; negative cases live in separate arrays
  (`decode_errors`, `error_cases`, `reject_cases`, `identity_errors`, `negative`). Every entry
  has a `"name"` so test harnesses can report per-case results. A loader that only understands
  `"valid"` should treat "has an `error` field" as invalid and "has no `error` field" as valid.
* CBOR decoders MUST tolerate unknown map keys but MUST reject missing required keys
  (PROTOCOL.md §16).

## Files

### `frame.json`

Frame header encode/decode (PROTOCOL.md §4).

* `encode[]`: `type`, `type_name`, `flags` (low nibble of byte 1), `flag_names`, `ttl`,
  `src_hex`, `dst_hex`, `seq` (u32 BE), `body_hex`, `frame_hex` (header + body), `aad_hex`.
  `aad_hex` is the 22-byte header with the **TTL nibble cleared**; that is what the AEAD
  authenticates (relays decrement TTL in flight).
* `decode[]`: `frame_hex` plus `expect` in the same shape. Parsed frames must match every
  field, including `aad_hex`.
* `decode_errors[]`: `frame_hex`, `error`.

### `identity.json`

PROTOCOL.md §3. `nodes[]`: `seed_hex` -> `node_id_hex`, `sign_pk_hex`, `dh_pk_hex`,
`public_identity_hex` (72 bytes: `node_id || sign_pk || dh_pk`).
Node ids are unicast: **bit 7 of byte 0 is cleared**. `channels[]` shows channel ids with that
bit **set**. `signatures[]` are deterministic Ed25519 vectors. `dm_keys[]` shows the
X25519+HKDF direct-message key both sides derive. `identity_errors[]` are parse/self-consistency
negative cases (wrong length -> `Malformed`; a tampered node id parses but
`self_consistent: false`).

### `seal.json`

PROTOCOL.md §5. `dm[]` / `channel[]` cases give a fixed frame header (`aad_hex`), plaintext, a
**test-only** `nonce_prefix_hex`, and the resulting `sealed_hex`
(`nonce_prefix || ct || tag`). Production MUST use a CSPRNG nonce prefix; the fixed prefix
exists only so this vector is reproducible. `body_layouts[]` gives complete sealed bodies:
`payload_flags(1) || msg_ref(4 BE) || [identity(72) if bit 0] || sealed`.
`ack_body` uses `payload_flags = 2`; `manifest_body` uses `payload_flags = 4`.
`negative[]` is a tampered ciphertext that must fail to open (`Decrypt`).

`StoreOffer` and `StoreWant` frame bodies are **clear-text CBOR** (`{ids: [...], count}` /
`{ids: [...]}`) with the `PRIORITY` header flag and no `ENC` bit: a relay may hold envelopes for
an addressee whose announce it never heard, so it may have no key material to seal with, and
the envelope ids are already clear-text `msg_ref`s in the retained Data/Resource frames.

### `announce.json`

PROTOCOL.md §6. Fixed binary announce body (NOT CBOR):

```
version(1) | class(1) | caps(2 BE) | reserved(1, MUST be 0) | name_len(1) |
interval_s(2 BE) | timestamp_ms(8 BE) | node_id(8) | sign_pk(32) | dh_pk(32) |
name(name_len) | signature(64)
```

`cases[]`: `announce` (decoded fields), `body_without_signature_hex` (the exact bytes signed
after the domain prefix), `signature_hex`, `body_hex`, and a `relayed_ttl7` frame showing that
relays only decrement TTL. **`hops` is not on the wire**: offset 4 is reserved zero and the
receiver derives `hops = ANNOUNCE_TTL(8) - frame.ttl`. Relays MUST NOT modify the body or the
signature. `reject_cases[]`: tampered signature, non-zero reserved byte, truncated body, wrong
version.

Signature payload: `"openmesh/v1/announce" || src(8) || seq(4 BE) || body_without_signature`.

### `app_message.json`

PROTOCOL.md §5.3/§7. `cases[]`: `message` (readable JSON; byte fields use `_hex`), `cbor_hex`
(the exact plaintext bytes to seal). `ack[]` gives CBOR-encoded `Ack` payloads, including a
`resource_window` example whose bitmap semantics are `bit 0 = next_expected + 1`. Optional
fields are omitted when absent.

### `resource.json`

PROTOCOL.md §9. `blob` is a complete worked example:

1. `plaintext_hex` is the payload.
2. `aad_hex` = `"openmesh/v1/resource" || res_id(4 BE) || msg_id(4 BE) || total_len(4 BE) ||
   chunk_bytes(2 BE)` — **34 bytes**. (An earlier spec revision said "(25 bytes)"; that
   annotation was an arithmetic error, the formula is authoritative.)
3. `sealed_blob_hex` = `payload_flags(1) = 0 || nonce_prefix(12) || ct || tag`;
   `total_len` = length of that blob.
4. `sha256_hex` is over the whole sealed blob.
5. `manifest.cbor_hex` is the CBOR `ResourceManifest` carried by a `Data` frame with
   `payload_flags` bit 2 (`IS_MANIFEST`) set.
6. `chunks[]`: each resource-frame body is
   `payload_flags(1)=0 || msg_ref(4 BE) || res_id(4 BE) || chunk_index(2 BE) || slice`.
7. `ack_bitmaps[]` pin selective-repeat ack semantics: `next_expected` is the first missing
   chunk; `bitmap` bit `i` reports chunk `next_expected + 1 + i`.
8. `blob.total_len` = `1 + 12 + plaintext_len + 16`; `chunk_count = ceil(total_len / chunk_bytes)`.

### `companion.json`

docs/architecture/COMPANION-API.md. `cases[]`: `type` (u8), `payload` (readable JSON),
`payload_cbor_hex`, `frame_hex` (`"OM" | type | len(u16 LE) | payload | crc16(u16 LE)`,
CRC-16/CCITT-FALSE over `type || len || payload`), `slip_hex` (SLIP delimiters and escapes).
`error_cases[]` cover short header, bad magic, unknown type, bad CRC, oversize and partial
frames.

## Regenerating

```sh
cargo run -p openmesh-core --example gen_vectors
cargo test -p openmesh-core
```
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vectors_are_current() {
        let expected_dir = default_vectors_dir();
        let actual: Vec<(&'static str, String)> = generate_files();
        assert_eq!(
            actual.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
            VECTOR_FILES.to_vec(),
            "VECTOR_FILES must list every generated file"
        );
        for (name, contents) in actual {
            let path = expected_dir.join(name);
            let committed = fs::read_to_string(&path).unwrap_or_else(|_| {
                panic!(
                    "missing vector file {}; run `cargo run -p openmesh-core --example gen_vectors`",
                    path.display()
                )
            });
            assert_eq!(
                committed,
                contents,
                "vector file {} is stale; run `cargo run -p openmesh-core --example gen_vectors`",
                path.display()
            );
        }
    }

    #[test]
    fn generation_is_deterministic() {
        let first = generate_files();
        let second = generate_files();
        assert_eq!(first, second);
    }

    #[test]
    fn vector_fixtures_are_self_consistent() {
        // The identity fixtures really are the identities derived from the seeds.
        let identity = identity_vectors();
        let nodes = identity["nodes"].as_array().expect("nodes");
        for node in nodes {
            let seed: Vec<u8> = hex::decode(node["seed_hex"].as_str().expect("seed")).unwrap();
            let seed: [u8; 32] = seed.try_into().unwrap();
            let ident = Identity::from_seed(&seed).public();
            assert_eq!(node["node_id_hex"], hex(&ident.node_id.0));
            assert_eq!(node["public_identity_hex"], hex(&ident.to_bytes()));
        }
        // The announce fixtures verify with the announced key.
        let announce = announce_vectors();
        for case in announce["cases"].as_array().expect("cases") {
            let body = hex::decode(case["body_hex"].as_str().expect("body")).unwrap();
            let (decoded, signature) = Announce::decode_signed(&body).expect("announce parses");
            let unsigned_len = body.len() - services::ANNOUNCE_SIG_LEN;
            let seq = case["seq"].as_u64().expect("seq") as u32;
            let payload =
                announce_signature_payload(&decoded.ident.node_id, seq, &body[..unsigned_len]);
            decoded
                .ident
                .verify(&payload, &signature)
                .expect("signature verifies");
        }
        // The resource fixture's manifest hash covers the whole sealed blob.
        let resource = resource_vectors();
        let blob = hex::decode(resource["blob"]["sealed_blob_hex"].as_str().unwrap()).unwrap();
        let digest: [u8; 32] = Sha256::digest(&blob).into();
        assert_eq!(resource["blob"]["sha256_hex"], hex(&digest));
        assert_eq!(resource["blob"]["aad_len"], 34);
        // The companion fixtures decode with the reference decoder.
        let companion = companion_vectors();
        for case in companion["cases"].as_array().expect("cases") {
            let frame = hex::decode(case["frame_hex"].as_str().unwrap()).unwrap();
            let (decoded, consumed) = crate::companion::decode(&frame).expect("companion decodes");
            assert_eq!(consumed, frame.len());
            let slip = hex::decode(case["slip_hex"].as_str().unwrap()).unwrap();
            let (slip_decoded, _) = crate::companion::decode_slip(&slip).expect("slip decodes");
            assert_eq!(decoded, slip_decoded);
        }
    }
}
