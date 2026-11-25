// SPDX-License-Identifier: AGPL-3.0-or-later
// Test entry point: works under PlatformIO Unity (`pio test -e native`) and the local host
// runner built with zig/gcc (firmware/ci/host_tests.sh).
#include "tests.h"

#ifdef OM_USE_UNITY
#include <unity.h>
#else
#include "test_shim.h"
#endif

int main(int argc, char** argv) {
    (void)argc;
    (void)argv;
#ifdef OM_USE_UNITY
    UNITY_BEGIN();
#endif

    // Crypto / encodings
    RUN_TEST(test_sha256_vectors);
    RUN_TEST(test_hmac_sha256_vectors);
    RUN_TEST(test_hkdf_sha256_vectors);
    RUN_TEST(test_ed25519_rfc8032);
    RUN_TEST(test_x25519_rfc7748);
    RUN_TEST(test_identity_node_id_address_class);
    RUN_TEST(test_channel_key_and_id);
    RUN_TEST(test_seed_at_rest_roundtrip);

    // Frame codec
    RUN_TEST(test_frame_known_bytes);
    RUN_TEST(test_frame_aad_ignores_ttl);
    RUN_TEST(test_frame_decode_rejects);
    RUN_TEST(test_frame_relay_ttl);
    RUN_TEST(test_sealed_body_layout);

    // Announce
    RUN_TEST(test_announce_roundtrip_and_verify);
    RUN_TEST(test_announce_rejects_tampering);
    RUN_TEST(test_announce_hops_from_ttl);

    // Messages
    RUN_TEST(test_app_message_cbor_roundtrip);
    RUN_TEST(test_ack_cbor_roundtrip);
    RUN_TEST(test_manifest_cbor_roundtrip);
    RUN_TEST(test_store_offer_want_cbor);
    RUN_TEST(test_group_message_roundtrip);

    // Services
    RUN_TEST(test_dedup_cache);
    RUN_TEST(test_store_queue);
    RUN_TEST(test_store_serialization);
    RUN_TEST(test_airtime_and_budget);
    RUN_TEST(test_gps_nmea);
    RUN_TEST(test_battery_ocv);
    RUN_TEST(test_companion_chunk_roundtrip);
    RUN_TEST(test_companion_slip_escaping);
    RUN_TEST(test_companion_crc_rejects_corruption);
    RUN_TEST(test_resource_blob_roundtrip);

    // Engine integration
    RUN_TEST(test_engine_announce_and_peer_book);
    RUN_TEST(test_engine_direct_message_and_ack);
    RUN_TEST(test_engine_relay_decrements_ttl);
    RUN_TEST(test_engine_channel_position);
    RUN_TEST(test_engine_store_forward_handshake);
    RUN_TEST(test_engine_config_and_companion_views);

    // Golden vectors
    RUN_TEST(test_vectors_frame);
    RUN_TEST(test_vectors_identity);
    RUN_TEST(test_vectors_announce);
    RUN_TEST(test_vectors_seal);
    RUN_TEST(test_vectors_app_message);
    RUN_TEST(test_vectors_resource);
    RUN_TEST(test_vectors_companion);

#ifdef OM_USE_UNITY
    return UNITY_END();
#else
    std::printf("\n%d test case(s), %d assertion(s), %d failure(s)\n", omTestsRun,
                omAssertions, omTestsFailed);
    if (omTestsFailed == 0) std::printf("RESULT: PASS\n");
    else std::printf("RESULT: FAIL\n");
    return omTestsFailed == 0 ? 0 : 1;
#endif
}
