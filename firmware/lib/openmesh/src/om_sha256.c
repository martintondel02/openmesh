// SPDX-License-Identifier: AGPL-3.0-or-later
// SHA-256 / HMAC-SHA256 / HKDF-SHA256 (FIPS 180-4, RFC 2104, RFC 5869).
// Portable C99, no dynamic allocation, constant-workspace.

#include "openmesh/om_sha256.h"

#include <string.h>

static const uint32_t K256[64] = {
    0x428a2f98u, 0x71374491u, 0xb5c0fbcfu, 0xe9b5dba5u, 0x3956c25bu, 0x59f111f1u, 0x923f82a4u,
    0xab1c5ed5u, 0xd807aa98u, 0x12835b01u, 0x243185beu, 0x550c7dc3u, 0x72be5d74u, 0x80deb1feu,
    0x9bdc06a7u, 0xc19bf174u, 0xe49b69c1u, 0xefbe4786u, 0x0fc19dc6u, 0x240ca1ccu, 0x2de92c6fu,
    0x4a7484aau, 0x5cb0a9dcu, 0x76f988dau, 0x983e5152u, 0xa831c66du, 0xb00327c8u, 0xbf597fc7u,
    0xc6e00bf3u, 0xd5a79147u, 0x06ca6351u, 0x14292967u, 0x27b70a85u, 0x2e1b2138u, 0x4d2c6dfcu,
    0x53380d13u, 0x650a7354u, 0x766a0abbu, 0x81c2c92eu, 0x92722c85u, 0xa2bfe8a1u, 0xa81a664bu,
    0xc24b8b70u, 0xc76c51a3u, 0xd192e819u, 0xd6990624u, 0xf40e3585u, 0x106aa070u, 0x19a4c116u,
    0x1e376c08u, 0x2748774cu, 0x34b0bcb5u, 0x391c0cb3u, 0x4ed8aa4au, 0x5b9cca4fu, 0x682e6ff3u,
    0x748f82eeu, 0x78a5636fu, 0x84c87814u, 0x8cc70208u, 0x90befffau, 0xa4506cebu, 0xbef9a3f7u,
    0xc67178f2u,
};

static uint32_t rotr32(uint32_t x, unsigned n) { return (x >> n) | (x << (32 - n)); }

static void sha256_transform(uint32_t state[8], const uint8_t block[64]) {
    uint32_t w[64];
    for (int i = 0; i < 16; ++i) {
        w[i] = ((uint32_t)block[i * 4] << 24) | ((uint32_t)block[i * 4 + 1] << 16) |
               ((uint32_t)block[i * 4 + 2] << 8) | (uint32_t)block[i * 4 + 3];
    }
    for (int i = 16; i < 64; ++i) {
        uint32_t s0 = rotr32(w[i - 15], 7) ^ rotr32(w[i - 15], 18) ^ (w[i - 15] >> 3);
        uint32_t s1 = rotr32(w[i - 2], 17) ^ rotr32(w[i - 2], 19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16] + s0 + w[i - 7] + s1;
    }
    uint32_t a = state[0], b = state[1], c = state[2], d = state[3];
    uint32_t e = state[4], f = state[5], g = state[6], h = state[7];
    for (int i = 0; i < 64; ++i) {
        uint32_t s1 = rotr32(e, 6) ^ rotr32(e, 11) ^ rotr32(e, 25);
        uint32_t ch = (e & f) ^ ((~e) & g);
        uint32_t t1 = h + s1 + ch + K256[i] + w[i];
        uint32_t s0 = rotr32(a, 2) ^ rotr32(a, 13) ^ rotr32(a, 22);
        uint32_t maj = (a & b) ^ (a & c) ^ (b & c);
        uint32_t t2 = s0 + maj;
        h = g;
        g = f;
        f = e;
        e = d + t1;
        d = c;
        c = b;
        b = a;
        a = t1 + t2;
    }
    state[0] += a;
    state[1] += b;
    state[2] += c;
    state[3] += d;
    state[4] += e;
    state[5] += f;
    state[6] += g;
    state[7] += h;
}

void om_sha256_init(om_sha256_ctx* ctx) {
    ctx->state[0] = 0x6a09e667u;
    ctx->state[1] = 0xbb67ae85u;
    ctx->state[2] = 0x3c6ef372u;
    ctx->state[3] = 0xa54ff53au;
    ctx->state[4] = 0x510e527fu;
    ctx->state[5] = 0x9b05688cu;
    ctx->state[6] = 0x1f83d9abu;
    ctx->state[7] = 0x5be0cd19u;
    ctx->total_len = 0;
    ctx->buf_len = 0;
}

static void sha256_push(om_sha256_ctx* ctx, const uint8_t* data, size_t len) {
    ctx->total_len += (uint64_t)len;
    while (len > 0) {
        size_t room = 64 - ctx->buf_len;
        size_t take = len < room ? len : room;
        memcpy(ctx->buf + ctx->buf_len, data, take);
        ctx->buf_len += take;
        data += take;
        len -= take;
        if (ctx->buf_len == 64) {
            sha256_transform(ctx->state, ctx->buf);
            ctx->buf_len = 0;
        }
    }
}

void om_sha256_update(om_sha256_ctx* ctx, const void* data, size_t len) {
    sha256_push(ctx, (const uint8_t*)data, len);
}

void om_sha256_final(om_sha256_ctx* ctx, uint8_t out[32]) {
    uint64_t bit_len = ctx->total_len * 8u;
    uint8_t pad[72];
    size_t pad_len = (ctx->buf_len < 56) ? (56 - ctx->buf_len) : (120 - ctx->buf_len);
    memset(pad, 0, sizeof(pad));
    pad[0] = 0x80;
    for (int i = 0; i < 8; ++i) pad[pad_len + i] = (uint8_t)(bit_len >> (56 - 8 * i));
    sha256_push(ctx, pad, pad_len + 8);
    for (int i = 0; i < 8; ++i) {
        out[i * 4] = (uint8_t)(ctx->state[i] >> 24);
        out[i * 4 + 1] = (uint8_t)(ctx->state[i] >> 16);
        out[i * 4 + 2] = (uint8_t)(ctx->state[i] >> 8);
        out[i * 4 + 3] = (uint8_t)ctx->state[i];
    }
    memset(ctx, 0, sizeof(*ctx));
}

void om_sha256(const void* data, size_t len, uint8_t out[32]) {
    om_sha256_ctx ctx;
    om_sha256_init(&ctx);
    om_sha256_update(&ctx, data, len);
    om_sha256_final(&ctx, out);
}

// --- HMAC-SHA256 ---------------------------------------------------------------------------
void om_hmac_sha256_init(om_hmac_sha256_ctx* ctx, const void* key, size_t key_len) {
    uint8_t k[64];
    uint8_t ipad[64];
    uint8_t opad[64];
    memset(k, 0, sizeof(k));
    if (key_len > sizeof(k)) {
        om_sha256(key, key_len, k);
    } else {
        memcpy(k, key, key_len);
    }
    for (int i = 0; i < 64; ++i) {
        ipad[i] = (uint8_t)(k[i] ^ 0x36);
        opad[i] = (uint8_t)(k[i] ^ 0x5C);
    }
    om_sha256_init(&ctx->inner);
    om_sha256_update(&ctx->inner, ipad, sizeof(ipad));
    om_sha256_init(&ctx->outer);
    om_sha256_update(&ctx->outer, opad, sizeof(opad));
    memset(k, 0, sizeof(k));
    memset(ipad, 0, sizeof(ipad));
    memset(opad, 0, sizeof(opad));
}

void om_hmac_sha256_update(om_hmac_sha256_ctx* ctx, const void* data, size_t len) {
    om_sha256_update(&ctx->inner, data, len);
}

void om_hmac_sha256_final(om_hmac_sha256_ctx* ctx, uint8_t out[32]) {
    uint8_t inner[32];
    om_sha256_final(&ctx->inner, inner);
    om_sha256_update(&ctx->outer, inner, sizeof(inner));
    om_sha256_final(&ctx->outer, out);
    memset(inner, 0, sizeof(inner));
}

void om_hmac_sha256(const void* key, size_t key_len, const void* data, size_t data_len,
                    uint8_t out[32]) {
    om_hmac_sha256_ctx ctx;
    om_hmac_sha256_init(&ctx, key, key_len);
    om_hmac_sha256_update(&ctx, data, data_len);
    om_hmac_sha256_final(&ctx, out);
}

// --- HKDF-SHA256 ---------------------------------------------------------------------------
void om_hkdf_sha256(const void* salt, size_t salt_len, const void* ikm, size_t ikm_len,
                    const void* info, size_t info_len, uint8_t* out, size_t out_len) {
    // Extract
    uint8_t prk[32];
    om_hmac_sha256(salt, salt_len, ikm, ikm_len, prk);

    // Expand
    uint8_t t[32];
    size_t t_len = 0;
    size_t written = 0;
    uint8_t counter = 1;
    while (written < out_len) {
        om_hmac_sha256_ctx ctx;
        om_hmac_sha256_init(&ctx, prk, sizeof(prk));
        if (t_len > 0) om_hmac_sha256_update(&ctx, t, t_len);
        if (info_len > 0) om_hmac_sha256_update(&ctx, info, info_len);
        om_hmac_sha256_update(&ctx, &counter, 1);
        om_hmac_sha256_final(&ctx, t);
        t_len = sizeof(t);
        size_t take = (out_len - written) < sizeof(t) ? (out_len - written) : sizeof(t);
        memcpy(out + written, t, take);
        written += take;
        ++counter;
    }
    memset(prk, 0, sizeof(prk));
    memset(t, 0, sizeof(t));
}
