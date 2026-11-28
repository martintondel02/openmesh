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
    "tiny":   { "max_dim": 256,  "quality": 0.28, "mime": "image/avif" },
    "medium": { "max_dim": 512,  "quality": 0.40, "mime": "image/avif" },
    "high":   { "max_dim": 1024, "quality": 0.50, "mime": "image/avif" },
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

Encoder policy for image tiers:

* **Auto: send the smaller of AVIF and WebP at matched visual quality.** The bundled Rust AVIF
  encoder (`crates/openmesh-image` via `openmesh-image-wasm` on web and `openmesh-ffi` on Android)
  is preferred where it produces a **strictly smaller** file than canvas/platform WebP; a tie goes
  to WebP. AVIF is encoded at the quality-calibrated setting (§1.2), not at the preset's numeric
  WebP quality. If the AVIF encoder is unavailable or errors, the client sends WebP (or JPEG when
  WebP is unavailable) and labels the attachment with the **actual** encoded MIME and matching
  filename extension. A requested type that comes back as something else must never be sent under
  the requested name.
* `mime` in the preset is a **preference/indication**, not a mandate: clients apply the Auto
  selection above. `original` passes the picked bytes through unchanged (the client does not
  transcode or relabel it).

Rules:

* **Image tiers** are targets: clients downscale to `max_dim`, encode quality-matched AVIF and
  WebP candidates where the encoders are available, send the strictly smaller result (WebP on a
  tie; JPEG only if WebP cannot be produced), then show the *actual* bytes, actual MIME and
  airtime. If the result exceeds the active profile's attachment cap
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

### 1.1 Measured encoder output (2026-11-28)

Real browser measurement (Chromium 159 headless, demo mode) on a 768×512 Kodak test photo:
canvas downscale to the preset `max_dim`, then both candidates — AVIF through the bundled WASM
Worker at the calibrated quality (§1.2) and WebP through canvas at the preset quality — encoded
from the same canvas. Bytes are the actual uploaded payloads; timings are Worker-side encode time
on a desktop (phone browsers will be slower). No relays/retries are included.

| Tier | Output | AVIF (WASM, calibrated q) | WebP (canvas) | JPEG (canvas) | Chosen |
| --- | --- | --- | --- | --- | --- |
| tiny | 256×171 | 2,642 B (q35, speed 10, 54 ms) | 3,066 B (q28) | 3,940 B | AVIF |
| medium | 512×341 | 7,468 B (q50, speed 8, 227 ms) | 8,714 B (q40) | 12,318 B | AVIF |
| high | 768×512 | 16,470 B (q62, speed 8, 664 ms) | 17,290 B (q50) | 26,633 B | AVIF |

The AVIF candidate wins these three samples by **14 % / 14 % / 5 %** at matched visual quality;
the same photo's WebP candidate is **29 % / 37 % / 22 % smaller** than the previous
320/640/1280 px presets. A text-heavy 2160×1350 UI screenshot shows the opposite at high:
AVIF 17,580 B (q62) vs WebP 16,648 B (q50) → **WebP was sent** (tiny/medium still chose AVIF).
That is the point of the dynamic choice (§1.2). These are content-dependent
examples: clients always display the measured bytes and the chosen format for the actual file.

Chromium's **canvas** still cannot encode AVIF (`toBlob('image/avif')` silently returns
`image/png`), so the web client never uses canvas AVIF; the AVIF candidate comes from the bundled
WASM encoder. On the web the module is ~1.3 MB raw / ~406 KB gzip-9, loaded lazily in a Worker on
the first image encode (not at app startup), once per page session. With the WASM artifact absent
or failing, the composer falls back to canvas WebP/JPEG and the attachment keeps that MIME
(verified headlessly: a static build without the WASM produced `image/webp`, never AVIF).
Real AVIF *decode* is supported by Chromium (a 12,877 B, 2048×858 sample renders via
`createImageBitmap`/`<img>`), and the web client renders received `image/avif` attachments in the
bubble, fullscreen viewer and Save dialog.

### 1.2 Quality-calibrated AVIF/WebP choice (2026-11-28)

The bundled pure-Rust AVIF encoder (`crates/openmesh-image`, ravif/rav1e) is now available through
`crates/openmesh-image-wasm` (web) and `openmesh-ffi` (`encodeAvif`, Android). Its **ravif quality
is perceptually lower than libwebp/platform WebP at the same numeric value**, so a same-q byte
comparison would often choose AVIF at lower visual quality. The policy is therefore
**Auto (smallest, quality-matched)**, not blind AVIF preference:

* Encode the tier WebP candidate at the preset quality (`0.28`/`0.40`/`0.50` → q28/40/50) and the
  AVIF candidate at [`openmesh_image::calibrated_avif_quality`] — the documented **1.25×**
  calibration, integer-floored and capped at 100: **tiny q28 → AVIF q35, medium q40 → AVIF q50,
  high q50 → AVIF q62**. (Constants/functions: `AVIF_QUALITY_CALIBRATION_NUMERATOR` /
  `DENOMINATOR` / `MAX`, `calibrated_avif_quality`, `calibrated_avif_quality_from_fraction`.)
* Send the **strictly smaller** result; **WebP wins ties**. AVIF is only chosen when it truly wins
  at comparable quality, and the central encoder mapping (q28/40/50) is unchanged.
* If the AVIF encoder is unavailable, over budget, or fails, use WebP (or JPEG) and label the
  attachment with the **actual** MIME/filename. Never send fallback bytes under an AVIF name.
* Composer default: **Auto (smallest, quality-matched)** with the actual format/bytes/airtime shown;
  explicit **force AVIF** / **force WebP** choices can be added later as user overrides.
* This policy is implemented in both clients: Android size-aware selection and the web lazy
  AVIF WASM path (Worker encode, AVIF/WebP comparison, WebP tie-break). The encoder
  API/capability contract above is unchanged.

**Measurement (SSIM-matched quality, 126 encodes).** Three real 2048×1536 sample photos
(Picsum/Unsplash: one typical scene, one portrait-style scene, one detail-heavy star-field
lighthouse), Lanczos-resized to the 256/512/1024 tier bounds. WebP via Pillow/libwebp method 6 at
q28/40/50; AVIF via `crates/openmesh-image` at q20–70 (tier speeds 10/8/8); decoded with
ffmpeg/libdav1d; SSIM computed against the resized reference. For each WebP point, the ravif quality
and byte size that match its SSIM were linearly interpolated between AVIF points. 126 = 3 photos ×
3 tiers × (3 WebP + 11 AVIF) encodes; SSIM is an engineering proxy, not a full perceptual metric.

Matching WebP SSIM needs roughly **1.2–1.3× the WebP quality** on typical photos (q28 → ravif
q39–41; q40 → q46–48; q50 → q54–55) and far more on detail-heavy content (up to q66–70+). At that
matched quality, AVIF bytes relative to WebP bytes were:

| Tier (preset WebP q) | typical photo A | typical photo B | detail-heavy lighthouse |
| --- | --- | --- | --- |
| tiny (q28) | **+19 %** | **−8 %** | **+133 %** |
| medium (q40) | **−10 %** | **−9 %** | **+86 %** |
| high (q50) | **−6 %** | **−8 %** | **+97 %** |

Negative means AVIF is smaller. Realistic expectation on typical photos: single-digit savings at
medium/high, roughly parity or slightly worse at tiny; AVIF-hostile detail-heavy photos can be
~2× larger, which the dynamic choice correctly sends as WebP. The earlier same-numeric-q
comparisons (e.g. “AVIF 21–31 % smaller”) overstated the equal-quality win because AVIF's quality
was lower at the same number. Re-check these numbers with device-side logs on representative
photos; the platform WebP encoder and content mix can shift them.

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

* Composer image attach: tier menu **Tiny (≤256 px, q0.28) / Medium (≤512 px, q0.40) / High
  (≤1024 px, q0.50) / Original**, with an **Auto** encoder label; the client encodes AVIF in the
  lazy WASM Worker and WebP on canvas, sends the strictly smaller result (WebP tie-break), and
  labels the tile/metrics with the actual format and bytes. The pre-send preview shows **actual
  size**, **actual MIME**, **estimated airtime**, **% of hourly duty**, and a warning when it
  exceeds the profile cap; `Original` requires a confirmation and is disabled by default for
  `LONG_*`.
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

* Image attach: tier chooser (Tiny ≤256 px / Medium ≤512 px / High ≤1024 px / Original) →
  compress with the platform image encoder (AVIF first when it genuinely emits `image/avif`, else
  WebP, then JPEG; the attachment MIME follows the actual bytes) → preview with actual bytes,
  MIME and airtime + duty %; `Original` gated.
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
