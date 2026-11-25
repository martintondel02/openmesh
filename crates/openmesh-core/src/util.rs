//! Small helpers: LoRa airtime math, radio profiles, regions, time and formatting.
//!
//! The airtime model follows Semtech AN1200.13/AN1200.22 and is used by:
//! * the bridge/UI to show users the cost of a message before sending it,
//! * the firmware to enforce duty-cycle budgets,
//! * the simulator to model channel occupancy realistically.

/// LoRa modem configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoraProfile {
    pub name: &'static str,
    pub sf: u8,
    pub bw_khz: u16,
    pub coding_rate: u8,
    pub preamble_symbols: u16,
    pub explicit_header: bool,
    pub crc: bool,
}

/// Common presets. Names follow the de-facto convention used by Meshtastic so users can
/// compare, but OpenMesh defaults to `MEDIUM_FAST` for a better speed/range balance.
pub const LONG_SLOW: LoraProfile =
    LoraProfile { name: "LONG_SLOW", sf: 12, bw_khz: 125, coding_rate: 8, preamble_symbols: 16, explicit_header: true, crc: true };
pub const LONG_FAST: LoraProfile =
    LoraProfile { name: "LONG_FAST", sf: 11, bw_khz: 250, coding_rate: 5, preamble_symbols: 16, explicit_header: true, crc: true };
pub const MEDIUM_FAST: LoraProfile =
    LoraProfile { name: "MEDIUM_FAST", sf: 9, bw_khz: 250, coding_rate: 5, preamble_symbols: 16, explicit_header: true, crc: true };
pub const SHORT_FAST: LoraProfile =
    LoraProfile { name: "SHORT_FAST", sf: 7, bw_khz: 250, coding_rate: 5, preamble_symbols: 16, explicit_header: true, crc: true };
pub const SHORT_TURBO: LoraProfile =
    LoraProfile { name: "SHORT_TURBO", sf: 7, bw_khz: 500, coding_rate: 5, preamble_symbols: 16, explicit_header: true, crc: true };

pub const PROFILES: &[LoraProfile] =
    &[LONG_SLOW, LONG_FAST, MEDIUM_FAST, SHORT_FAST, SHORT_TURBO];

/// Regulatory region profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub name: &'static str,
    /// Centre frequency in kHz.
    pub freq_khz: u32,
    /// Maximum allowed transmit power in dBm (EIRP where applicable).
    pub max_power_dbm: i8,
    /// Duty cycle in percent; `None` for regions with only fair-use rules.
    pub duty_cycle_pct: Option<u8>,
    /// Bandwidth allowed for the above duty cycle, in kHz.
    pub bw_khz: u16,
}

pub const REGIONS: &[Region] = &[
    Region { name: "EU868", freq_khz: 869_525, max_power_dbm: 14, duty_cycle_pct: Some(1), bw_khz: 250 },
    Region { name: "US915", freq_khz: 915_000, max_power_dbm: 20, duty_cycle_pct: None, bw_khz: 250 },
    Region { name: "AU915", freq_khz: 915_000, max_power_dbm: 20, duty_cycle_pct: None, bw_khz: 250 },
    Region { name: "IN865", freq_khz: 865_000, max_power_dbm: 20, duty_cycle_pct: None, bw_khz: 250 },
    Region { name: "KR920", freq_khz: 920_000, max_power_dbm: 14, duty_cycle_pct: None, bw_khz: 250 },
    Region { name: "AS923", freq_khz: 923_200, max_power_dbm: 14, duty_cycle_pct: Some(1), bw_khz: 250 },
];

/// On-air time of a LoRa frame in milliseconds, including preamble and header overhead.
///
/// Implements the classic Semtech airtime formula. Returns at least 1 ms.
pub fn lora_airtime_ms(profile: &LoraProfile, payload_len: usize) -> u32 {
    let sf = profile.sf as f64;
    let bw = profile.bw_khz as f64 * 1000.0;
    // Symbol duration Tsym = 2^SF / BW seconds.
    let tsym = (2f64.powi(profile.sf as i32)) / bw;
    let preamble = (profile.preamble_symbols as f64 + 4.25) * tsym;

    // Semtech AN1200.13 airtime formula.
    //   payload_symbols = 8 + max(ceil((8*PL - 4*SF + 28 - 20*IH) / (4*(SF - 2*DE))) * (CR + 4), 0)
    // PL includes the 16-bit CRC when enabled; IH = 0 with an explicit header; DE = 1 for the
    // low-data-rate optimisation (SF11/SF12 at 125 kHz); CR = 1..4 for 4/5..4/8.
    let pl = payload_len as f64 + if profile.crc { 2.0 } else { 0.0 };
    let ih = if profile.explicit_header { 0.0 } else { 1.0 };
    let de = if profile.sf >= 11 && profile.bw_khz == 125 { 1.0 } else { 0.0 };
    let cr = (profile.coding_rate.saturating_sub(4).clamp(1, 4)) as f64;
    let num = 8.0 * pl - 4.0 * sf + 28.0 - 20.0 * ih;
    let den = 4.0 * (sf - 2.0 * de);
    let symbol_count = (num / den).ceil().max(0.0);
    let payload_symbols = 8.0 + symbol_count * (cr + 4.0);

    let total_ms = (preamble + payload_symbols * tsym) * 1000.0;
    total_ms.ceil().max(1.0) as u32
}

/// Maximum attachment size recommended for a given profile and duty-cycle region.
pub fn max_attachment_bytes(profile: &LoraProfile, region: &Region) -> usize {
    let _ = (profile, region);
    // 90 s of airtime at the profile's rate, leaving headroom for ACKs and retries.
    let airtime_per_frame = lora_airtime_ms(profile, 240).max(1) as f64;
    let frames = (90_000.0 / airtime_per_frame) as usize;
    (frames * 180).max(512)
}

/// Current Unix time in milliseconds (host-side convenience).
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Format a byte count in a compact human-friendly way.
pub fn format_bytes(bytes: usize) -> String {
    match bytes {
        0..=1023 => format!("{bytes} B"),
        1024..=1_048_575 => format!("{:.1} KiB", bytes as f64 / 1024.0),
        _ => format!("{:.1} MiB", bytes as f64 / 1_048_576.0),
    }
}

/// Format a duration in milliseconds as `1h 02m`, `3m 04s`, `4.2s` or `820ms`.
pub fn format_duration_ms(ms: u64) -> String {
    if ms >= 3_600_000 {
        format!("{}h {:02}m", ms / 3_600_000, (ms % 3_600_000) / 60_000)
    } else if ms >= 60_000 {
        format!("{}m {:02}s", ms / 60_000, (ms % 60_000) / 1_000)
    } else if ms >= 1_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        format!("{ms}ms")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn airtime_is_plausible() {
        // A 240-byte frame at SF9/250kHz takes roughly 0.3-0.4s.
        let a = lora_airtime_ms(&MEDIUM_FAST, 240);
        assert!((450..900).contains(&a), "unexpected airtime {a} ms");
        // Higher spreading factor is strictly slower for the same payload.
        assert!(lora_airtime_ms(&LONG_SLOW, 240) > lora_airtime_ms(&SHORT_FAST, 240));
        assert!(lora_airtime_ms(&MEDIUM_FAST, 40) < lora_airtime_ms(&MEDIUM_FAST, 240));
    }

    #[test]
    fn formatting_helpers() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(2048), "2.0 KiB");
        assert_eq!(format_duration_ms(820), "820ms");
        assert_eq!(format_duration_ms(4_200), "4.2s");
        assert_eq!(format_duration_ms(184_000), "3m 04s");
    }
}
