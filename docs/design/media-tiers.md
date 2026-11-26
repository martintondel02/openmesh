# Media tiers and tiny voice (Codec2) — v0.6.0 contract

**Status:** normative for v0.6.0 · **Applies to:** `crates/openmesh-voice` (new),
`crates/openmesh-voice-wasm` (new), `crates/openmesh-ffi`, `apps/web`, `apps/android`,
`content/presets/media.json` (new), `content/help/media.md`, `docs/architecture/PROTOCOL.md` §9.4,
`docs/design/android-ga.md`, README/CHANGELOG.
**Decisions:** Codec2 licence approved (2026-11-26); airtime-first tiers inspired by MeshChatX but
driven by the active radio profile; no protocol/wire change (attachments already carry MIME +
bytes; resources are unchanged).

> Goal: make **tiny voice and small images first-class** on a 1–10 % duty-cycle LoRa channel.
> The codec must run identically in the browser (WASM), on Android (UniFFI/JNI), and in Rust
> (bridge/CLI/tests). We use the pure-Rust [`codec2`](https://crates.io/crates/codec2) crate
> (LGPL-2.1-only AND MIT), not the C library, so there is no NDK/emscripten build step.

---

## 1. Shared tier definitions (`content/presets/media.json`)

Single source for web, Android and tests. The bridge does not need it at runtime.

```json
{
  "version": 1,
  "airtime_note": "Tier sizes are targets; clients show the measured bytes and estimated airtime before sending.",
  "images": {
    "tiny":   { "max_dim": 320,  "quality": 0.35, "mime": "image/webp" },
    "medium": { "max_dim": 640,  "quality": 0.55, "mime": "image/webp" },
    "high":   { "max_dim": 1280, "quality": 0.70, "mime": "image/webp" },
    "original": { "max_dim": null, "quality": null, "mime": "original" }
  },
  "voice": {
    "tiny": {
      "codec": "codec2", "mode": "1300", "mime": "audio/c2-1300",
      "sample_rate": 8000, "channels": 1, "frame_ms": 40,
      "bits_per_second": 1300, "max_duration_s": 30
    },
    "medium": {
      "codec": "codec2", "mode": "3200", "mime": "audio/c2-3200",
      "sample_rate": 8000, "channels": 1, "frame_ms": 20,
      "bits_per_second": 3200, "max_duration_s": 20
    },
    "high": {
      "codec": "opus", "mode": "opus", "mime": "audio/ogg; codecs=opus",
      "sample_rate": 48000, "channels": 1,
      "bits_per_second": 12000, "max_duration_s": 15
    }
  },
  "defaults": { "image": "medium", "voice": "tiny" }
}
```

Rules:

* **Image tiers** are targets: clients downscale to `max_dim`, encode WebP at `quality`, then show
  the *actual* bytes and airtime. If the result exceeds the active profile's attachment cap
  (`util::max_attachment_bytes`), the next smaller tier is suggested; `original` is allowed only
  with an explicit confirmation and is blocked by default on long-range profiles.
* **Voice tiny/medium** are **raw Codec2 frame streams** (no container): fixed frame sizes
  7 bytes (1300 mode, 320 samples = 40 ms) and 8 bytes (3200 mode, 160 samples = 20 ms), 8 kHz mono
  s16 PCM input/output. Decoders ignore a trailing partial frame. MIME carries the mode.
* **Voice high** is Opus: web/desktop use `audio/ogg; codecs=opus` recorded via `MediaRecorder`;
  Android uses OGG/Opus when the platform supports it (API 29+) and otherwise AAC/MP4
  (`audio/mp4`) with the same visual tier label. Both clients must play the formats they may
  receive (Opus/OGG, AAC/MP4, Codec2 raw).
* Duration caps: effective cap = `min(tier.max_duration_s, floor(profile_cap_bytes /
  bytes_per_second(tier)))`; show the cap and the estimated airtime before recording/sending.
* Airtime labels: "≈N s airtime at <PROFILE> · ~M % of the hourly duty budget" using the existing
  `util::lora_airtime_ms` math (240-byte frames incl. resource overhead).

## 2. Rust crate `openmesh-voice`

Public API (host, FFI and wasm wrap this; no IO):

```rust
pub enum VoiceTier { Tiny, Medium, High }
pub enum VoiceError { UnsupportedTier, EmptyAudio, TooLarge, Decode(String) }

pub struct VoiceFormat {
    pub tier: VoiceTier, pub codec: &'static str, pub mode: &'static str,
    pub mime: &'static str, pub sample_rate: u32, pub channels: u8,
    pub frame_ms: u32, pub bits_per_second: u32, pub max_duration_s: u32,
}
pub fn format(tier: VoiceTier) -> &'static VoiceFormat;
pub fn formats() -> &'static [VoiceFormat];
pub fn bytes_per_second(tier: VoiceTier) -> u32;
pub fn estimate_bytes(tier: VoiceTier, duration_ms: u64) -> usize;
pub fn pcm_bytes_for(tier: VoiceTier, duration_ms: u64) -> usize;
pub fn max_duration_s(tier: VoiceTier, profile_cap_bytes: usize) -> u32;

/// Encode 8 kHz mono s16le PCM to the tier's payload. For `High`, returns
/// `UnsupportedTier` (clients use the platform Opus/AAC recorder).
pub fn encode(tier: VoiceTier, pcm_s16le: &[u8]) -> Result<Vec<u8>, VoiceError>;
/// Decode a tiny/medium payload to s16le PCM. `High` returns `UnsupportedTier`.
pub fn decode(tier: VoiceTier, payload: &[u8]) -> Result<Vec<u8>, VoiceError>;
```

* Frame mapping: Tiny → `Codec2::new(MODE_1300)` (52 bits/frame, 7 bytes; 320 samples/40 ms).
  Medium → `MODE_3200` (64 bits/frame, 8 bytes; 160 samples/20 ms). Encode consumes whole frames
  only (drop a trailing partial PCM frame); decode tolerates a trailing partial payload frame.
* Input length must be a multiple of 2 bytes; non-multiple-of-frame PCM is truncated to whole
  frames. Empty input → `EmptyAudio`.
* Tests: sine/noise round-trip (decoded RMS > 0), byte-exact framing sizes for 1 s/8 s,
  `estimate_bytes`/`max_duration_s` math, empty/short/trailing-byte handling, High rejection.
* Crate license/NOTICE: dependency `codec2` is **LGPL-2.1-only AND MIT**; add to NOTICE with the
  source link and note that OpenMesh source covers LGPL relinking/rebuild obligations.

## 3. WASM wrapper `openmesh-voice-wasm`

`cdylib` compiled for `wasm32-unknown-unknown` (std allowed), C ABI for a minimal JS loader:

```c
uint8_t* om_voice_encode(uint32_t tier, const uint8_t* pcm, uint32_t len, uint32_t* out_len);
uint8_t* om_voice_decode(uint32_t tier, const uint8_t* payload, uint32_t len, uint32_t* out_len);
void     om_voice_free(uint8_t* ptr, uint32_t len);
uint32_t om_voice_frame_bytes(uint32_t tier);
```

`tier` 0 = tiny, 1 = medium, 2 = high (high rejected). Returns NULL on error, `*out_len = 0`.
Build script `scripts/build-voice-wasm.sh`:

```bash
rustup target add wasm32-unknown-unknown
cargo build -p openmesh-voice-wasm --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/openmesh_voice_wasm.wasm apps/web/src/voice/openmesh_voice.wasm
```

Output is gitignored; CI (web jobs) and local dev run the script before `pnpm build`/`dev`.
Web loader `apps/web/src/voice/codec2.ts`: instantiate via `?url`, malloc/copy, encode/decode,
free; unit test with a 440 Hz 1 s tone (round-trip > 0 samples, tiny ≤ ~200 B for 1 s).

## 4. FFI additions (Android)

```kotlin
// package uniffi.openmesh_ffi
fun openmeshVoiceEncode(tier: String, pcmS16le: ByteArray): ByteArray     // throws FfiException
fun openmeshVoiceDecode(tier: String, payload: ByteArray): ByteArray     // s16le PCM
fun openmeshVoiceTiersJson(): String                                     // media.json voice block
```
`tier` accepts `"tiny"`/`"medium"`/`"high"`; tiny/medium map to the crate, high throws
InvalidArgument. Errors use existing `FfiException.InvalidArgument` with `VoiceError` text.
Generated Kotlin names must be confirmed by the implementer and relayed to platform-docs.

## 5. Web UX (web-client)

* Composer image attach: tier menu **Tiny / Medium / High / Original** (with dims/quality and
  target size), compression, then a pre-send preview card showing **actual size**, **estimated
  airtime**, **% of hourly duty**, and a warning when it exceeds the profile cap; `Original`
  requires a confirmation and is disabled by default for `LONG_*`.
* Voice recorder: tier selector **Tiny (Codec2 1.3k) / Medium (Codec2 3.2k) / High (Opus)**;
  record ≤ effective cap; show bytes + airtime before send. Tiny/medium: capture PCM via
  `AudioContext`/`AudioWorklet` (or `MediaRecorder` decode), resample to 8 kHz mono via
  `OfflineAudioContext`, encode with WASM; playback: decode WASM → `AudioBuffer`. High: existing
  Opus/OGG recorder.
* Incoming: play Codec2 attachments by fetching bytes, decoding via WASM, `AudioBufferSource`;
  existing audio player shows the tier/format label ("Codec2 1.3k · 8 s · 1.4 KiB").
* Demo mode: seed tier metadata and a small synthetic Codec2 clip (generated from a tone with the
  WASM in a test/build script, or a checked-in tiny fixture ≤2 KiB) so the UI can be screenshotted
  offline; never call the bridge.
* Tests: tier selection/auto-suggest, airtime formatting, cap math, Original gating, recorder tier
  mapping, Codec2 wasm round-trip smoke, demo data. Existing 255 tests stay green.

## 6. Android UX (platform-docs)

* Image attach: tier chooser (Tiny 320 / Medium 640 / High 1280 / Original) → compress
  (`Bitmap` + WebP quality) → preview with actual bytes + airtime + duty %; `Original` gated.
* Voice: tier chooser; tiny/medium record with `AudioRecord` (8 kHz mono s16le) → FFI
  `openmeshVoiceEncode` → `sendAttachment(mime)`; high: existing MediaRecorder path (OGG/Opus when
  API ≥29, else AAC/MP4). Effective cap per tier/profile shown before recording.
* Playback: Codec2 attachments → FFI decode to PCM → `AudioTrack`; Opus/AAC via existing
  `MediaPlayer`. Incoming tier/format label in the bubble.
* Settings: default image/voice tier (from `media.json`), airtime preview on/off already exists.
* Tests: tier table parity with `content/presets/media.json`, cap math, mime/encoding mapping,
  FFI wrapper argument/exception handling, playback format dispatch, default settings
  persistence. `lintDebug testDebugUnitTest assembleDebug` green, APK reported.

## 7. Protocol, docs, release

* `docs/architecture/PROTOCOL.md` §9.4: add the voice MIMEs (`audio/c2-1300`, `audio/c2-3200`),
  raw frame layout/frame sizes, tier table and airtime examples; no wire change, vectors
  unchanged.
* `content/help/media.md`: rewrite around tiers, sizes, airtime costs, how to choose, and
  cross-client playback; keep beginner tone. Index unchanged (17 topics).
* README feature row + screenshot, CHANGELOG 0.6.0, version bump everywhere
  (workspace/web/Tauri/Android `versionCode` 7/firmware `OM_FW_VERSION`), NOTICE (codec2 licence).
* CI/release: run `scripts/build-voice-wasm.sh` before web build in `ci.yml` and `release.yml`
  web job; Android FFI build already compiles the new Rust dependency.
* Acceptance: Rust tests (incl. new voice crate), web build/tests + wasm smoke, Android
  lint/tests/APK with tiers, firmware unaffected, docs/help/OpenAPI green, golden vectors
  byte-identical, device QA of tiny/medium voice between web and Android (no radio needed for
  encode/decode; mesh delivery needs hardware).

## 8. Non-goals for v0.6

* Live voice calls (OMP/2 territory), video, adaptive multi-profile media channels.
* Codec2 700C (the pure-Rust crate implements 1200–3200; 1300 is our lowest tier).
* Opus encoding in the Rust crate (clients use platform codecs); no change to the attachment wire
  format or protocol version.
