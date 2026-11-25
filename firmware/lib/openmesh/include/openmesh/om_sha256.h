// SPDX-License-Identifier: AGPL-3.0-or-later
// Minimal SHA-256 / HMAC-SHA256 / HKDF-SHA256 used by OMP/1 (pure C, no dependencies).
//
// Monocypher (vendored under third_party/monocypher) provides Ed25519, X25519 and
// XChaCha20-Poly1305; it does not ship SHA-256, which OMP/1 needs for node ids, channel ids,
// resource hashes and HKDF. Validated against FIPS 180-4 / RFC 4231 / RFC 5869 vectors in
// firmware/test/test_protocol.
#ifndef OPENMESH_OM_SHA256_H
#define OPENMESH_OM_SHA256_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    uint32_t state[8];
    uint64_t total_len;
    uint8_t buf[64];
    size_t buf_len;
} om_sha256_ctx;

void om_sha256_init(om_sha256_ctx* ctx);
void om_sha256_update(om_sha256_ctx* ctx, const void* data, size_t len);
void om_sha256_final(om_sha256_ctx* ctx, uint8_t out[32]);
void om_sha256(const void* data, size_t len, uint8_t out[32]);

typedef struct {
    om_sha256_ctx inner;
    om_sha256_ctx outer;
} om_hmac_sha256_ctx;

void om_hmac_sha256_init(om_hmac_sha256_ctx* ctx, const void* key, size_t key_len);
void om_hmac_sha256_update(om_hmac_sha256_ctx* ctx, const void* data, size_t len);
void om_hmac_sha256_final(om_hmac_sha256_ctx* ctx, uint8_t out[32]);
void om_hmac_sha256(const void* key, size_t key_len, const void* data, size_t data_len,
                    uint8_t out[32]);

// HKDF-SHA256 (RFC 5869). `out_len` must be <= 255*32.
void om_hkdf_sha256(const void* salt, size_t salt_len, const void* ikm, size_t ikm_len,
                    const void* info, size_t info_len, uint8_t* out, size_t out_len);

#ifdef __cplusplus
}
#endif

#endif  // OPENMESH_OM_SHA256_H
