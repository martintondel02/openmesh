// SPDX-License-Identifier: AGPL-3.0-or-later
// Test declarations shared by the host/PlatformIO test runner.
#ifndef OPENMESH_TEST_DECLS_H
#define OPENMESH_TEST_DECLS_H

// Crypto and encodings.
void test_sha256_vectors();
void test_hmac_sha256_vectors();
void test_hkdf_sha256_vectors();
void test_ed25519_rfc8032();
void test_x25519_rfc7748();
void test_identity_node_id_address_class();
void test_channel_key_and_id();
void test_seed_at_rest_roundtrip();

// Frame codec.
void test_frame_known_bytes();
void test_frame_aad_ignores_ttl();
void test_frame_decode_rejects();
void test_frame_relay_ttl();
void test_sealed_body_layout();

// Announce.
void test_announce_roundtrip_and_verify();
void test_announce_rejects_tampering();
void test_announce_hops_from_ttl();

// Messages.
void test_app_message_cbor_roundtrip();
void test_ack_cbor_roundtrip();
void test_manifest_cbor_roundtrip();
void test_store_offer_want_cbor();
void test_group_message_roundtrip();

// Protocol services.
void test_dedup_cache();
void test_store_queue();
void test_store_serialization();
void test_airtime_and_budget();
void test_gps_nmea();
void test_battery_ocv();
void test_companion_chunk_roundtrip();
void test_companion_slip_escaping();
void test_companion_crc_rejects_corruption();
void test_resource_blob_roundtrip();

// Engine integration (multi-node, no radio).
void test_engine_announce_and_peer_book();
void test_engine_direct_message_and_ack();
void test_engine_relay_decrements_ttl();
void test_engine_channel_position();
void test_engine_store_forward_handshake();
void test_engine_config_and_companion_views();

// Regulatory policy (v0.5.0).
void test_region_table_v2();
void test_profile_bandwidth_validation();
void test_board_power_caps();
void test_effective_power_policy();
void test_duty_override_policy();
void test_frequency_override_and_config();
void test_regulatory_override_persistence();

// Golden-vector replay (protocol/vectors/*.json; skipped when absent).
void test_vectors_frame();
void test_vectors_identity();
void test_vectors_announce();
void test_vectors_seal();
void test_vectors_app_message();
void test_vectors_resource();
void test_vectors_companion();

#endif  // OPENMESH_TEST_DECLS_H
