//! Small helpers: LoRa airtime math, radio profiles, regions, time and formatting.
//!
//! The airtime model follows Semtech AN1200.13/AN1200.22 and is used by:
//! * the bridge/UI to show users the cost of a message before sending it,
//! * the firmware to enforce duty-cycle budgets,
//! * the simulator to model channel occupancy realistically.

/// LoRa modem configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoraProfile {
    /// Human-readable name.
    pub name: &'static str,
    /// Spreading factor (7..=12).
    pub sf: u8,
    /// Bandwidth in kHz.
    pub bw_khz: u16,
    /// Coding rate denominator (5..=8, i.e. 4/5 .. 4/8).
    pub coding_rate: u8,
    /// Preamble length in symbols.
    pub preamble_symbols: u16,
    /// Whether the LoRa packet uses an explicit header.
    pub explicit_header: bool,
    /// Whether the LoRa packet carries a CRC.
    pub crc: bool,
}

/// Common presets. Names follow the de-facto convention used by Meshtastic so users can
/// compare, but OpenMesh defaults to `MEDIUM_FAST` for a better speed/range balance.
pub const LONG_SLOW: LoraProfile = LoraProfile {
    name: "LONG_SLOW",
    sf: 12,
    bw_khz: 125,
    coding_rate: 8,
    preamble_symbols: 16,
    explicit_header: true,
    crc: true,
};
/// Long-range preset (SF11 / 250 kHz).
pub const LONG_FAST: LoraProfile = LoraProfile {
    name: "LONG_FAST",
    sf: 11,
    bw_khz: 250,
    coding_rate: 5,
    preamble_symbols: 16,
    explicit_header: true,
    crc: true,
};
/// Default OpenMesh preset (SF9 / 250 kHz).
pub const MEDIUM_FAST: LoraProfile = LoraProfile {
    name: "MEDIUM_FAST",
    sf: 9,
    bw_khz: 250,
    coding_rate: 5,
    preamble_symbols: 16,
    explicit_header: true,
    crc: true,
};
/// Short-range, fast preset (SF7 / 250 kHz).
pub const SHORT_FAST: LoraProfile = LoraProfile {
    name: "SHORT_FAST",
    sf: 7,
    bw_khz: 250,
    coding_rate: 5,
    preamble_symbols: 16,
    explicit_header: true,
    crc: true,
};
/// Fastest preset (SF7 / 500 kHz).
pub const SHORT_TURBO: LoraProfile = LoraProfile {
    name: "SHORT_TURBO",
    sf: 7,
    bw_khz: 500,
    coding_rate: 5,
    preamble_symbols: 16,
    explicit_header: true,
    crc: true,
};

/// All built-in profiles.
pub const PROFILES: &[LoraProfile] = &[LONG_SLOW, LONG_FAST, MEDIUM_FAST, SHORT_FAST, SHORT_TURBO];

/// Regulatory region profile.
///
/// Defaults are the **legal maxima** for the region (docs/design/regulatory-and-regions.md §1):
/// a region selection sets TX power to [`Region::max_power_dbm`] and the duty budget to
/// [`Region::duty_cycle_pct`] unless the user (or an active regulatory override, §9.5) chooses
/// something lower. [`Region::bw_khz`] is the **maximum legal bandwidth**, so profiles with a
/// wider bandwidth are not selectable in that region.
///
/// The power values are EIRP ceilings; a high-gain antenna can push effective radiated power
/// above them. Verify local rules — these values are not legal advice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    /// Human-readable name.
    pub name: &'static str,
    /// Centre frequency in kHz.
    pub freq_khz: u32,
    /// Maximum allowed transmit power in dBm (EIRP where applicable).
    pub max_power_dbm: i8,
    /// Duty cycle in percent; `None` for regions with only fair-use rules.
    pub duty_cycle_pct: Option<u8>,
    /// Maximum legal bandwidth in kHz (was "bandwidth allowed for the above duty cycle").
    pub bw_khz: u16,
}

impl Region {
    /// Whether `profile` is legal in this region (`profile.bw_khz <= region.bw_khz`).
    pub fn allows_profile(&self, profile: &LoraProfile) -> bool {
        profile.bw_khz <= self.bw_khz
    }

    /// The profile clients should fall back to when the desired one is too wide: `LONG_FAST`
    /// on the narrow 433 MHz band, `MEDIUM_FAST` everywhere else (both fit every region).
    pub fn fallback_profile(&self) -> &'static LoraProfile {
        if self.name == "EU433" {
            &LONG_FAST
        } else {
            &MEDIUM_FAST
        }
    }
}

/// Look up a built-in region by name (case-sensitive, as used in configs and the API).
pub fn region(name: &str) -> Option<&'static Region> {
    REGIONS.iter().find(|region| region.name == name)
}

/// Validate that `profile` may be used in `region`.
///
/// Rejects profiles whose bandwidth exceeds the region's maximum legal bandwidth, e.g.
/// `SHORT_TURBO` (500 kHz) in `EU868`, `EU433`, `IN865`, `KR920` and `AS923`.
pub fn validate_profile(region: &Region, profile: &LoraProfile) -> crate::error::Result<()> {
    if region.allows_profile(profile) {
        Ok(())
    } else {
        Err(crate::error::Error::Config(
            "profile bandwidth exceeds the region's maximum legal bandwidth",
        ))
    }
}

/// All built-in regulatory region profiles (v0.5.0 legal maxima; EU/Norway first).
///
/// Order is stable for CLI listings and setup recommendations: `EU868`, `EU433`, `US915`,
/// `AU915`, `IN865`, `KR920`, `AS923`. Look regions up by name with [`region`], not by index.
pub const REGIONS: &[Region] = &[
    Region {
        name: "EU868",
        freq_khz: 869_525,
        max_power_dbm: 27,
        duty_cycle_pct: Some(10),
        bw_khz: 250,
    },
    Region {
        name: "EU433",
        freq_khz: 433_920,
        max_power_dbm: 10,
        duty_cycle_pct: Some(10),
        bw_khz: 250,
    },
    Region {
        name: "US915",
        freq_khz: 915_000,
        max_power_dbm: 30,
        duty_cycle_pct: None,
        bw_khz: 500,
    },
    Region {
        name: "AU915",
        freq_khz: 915_000,
        max_power_dbm: 30,
        duty_cycle_pct: None,
        bw_khz: 500,
    },
    Region {
        name: "IN865",
        freq_khz: 865_000,
        max_power_dbm: 30,
        duty_cycle_pct: None,
        bw_khz: 250,
    },
    Region {
        name: "KR920",
        freq_khz: 920_000,
        max_power_dbm: 23,
        duty_cycle_pct: None,
        bw_khz: 250,
    },
    Region {
        name: "AS923",
        freq_khz: 923_200,
        max_power_dbm: 20,
        duty_cycle_pct: Some(10),
        bw_khz: 250,
    },
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
    let de = if profile.sf >= 11 && profile.bw_khz == 125 {
        1.0
    } else {
        0.0
    };
    let cr = (profile.coding_rate.saturating_sub(4).clamp(1, 4)) as f64;
    let num = 8.0 * pl - 4.0 * sf + 28.0 - 20.0 * ih;
    let den = 4.0 * (sf - 2.0 * de);
    let symbol_count = (num / den).ceil().max(0.0);
    let payload_symbols = 8.0 + symbol_count * (cr + 4.0);

    let total_ms = (preamble + payload_symbols * tsym) * 1000.0;
    total_ms.ceil().max(1.0) as u32
}

/// Maximum attachment size recommended for a given profile and duty-cycle region.
///
/// Takes the smaller of the PROTOCOL.md §9.4 default cap for the profile and a 90 s airtime
/// budget, so a `LONG_SLOW` link never queues more than a couple of minutes of channel time.
pub fn max_attachment_bytes(profile: &LoraProfile, region: &Region) -> usize {
    let cap = match profile.name {
        "SHORT_TURBO" | "SHORT_FAST" => 24 * 1024,
        "MEDIUM_FAST" => 12 * 1024,
        "LONG_FAST" => 6 * 1024,
        "LONG_SLOW" => 1_536,
        _ => 12 * 1024,
    };
    // 90 s of airtime at the profile's rate, leaving headroom for ACKs and retries.
    let airtime_per_frame = lora_airtime_ms(profile, 240).max(1) as f64;
    let frames = (90_000.0 / airtime_per_frame) as usize;
    let budget = (frames * 180).max(512);
    let mut limit = cap.min(budget);
    // Duty-cycle regions additionally clamp to one hour's worth of airtime.
    if let Some(pct) = region.duty_cycle_pct {
        let airtime_ms = 3_600_000u64 * pct as u64 / 100;
        let frames = (airtime_ms as f64 / airtime_per_frame) as usize;
        limit = limit.min((frames * 180).max(512));
    }
    limit
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
    fn region_table_matches_v050_contract() {
        // (name, centre kHz, max power dBm EIRP, duty %, max BW kHz)
        let expected = [
            ("EU868", 869_525, 27, Some(10), 250),
            ("EU433", 433_920, 10, Some(10), 250),
            ("US915", 915_000, 30, None, 500),
            ("AU915", 915_000, 30, None, 500),
            ("IN865", 865_000, 30, None, 250),
            ("KR920", 920_000, 23, None, 250),
            ("AS923", 923_200, 20, Some(10), 250),
        ];
        assert_eq!(
            REGIONS.len(),
            expected.len(),
            "region count changed without a contract bump"
        );
        for (index, (name, freq, power, duty, bw)) in expected.iter().enumerate() {
            let entry = &REGIONS[index];
            assert_eq!(
                entry.name, *name,
                "region order must stay stable for CLI listings"
            );
            assert_eq!(entry.freq_khz, *freq, "{name} centre");
            assert_eq!(entry.max_power_dbm, *power, "{name} max power");
            assert_eq!(entry.duty_cycle_pct, *duty, "{name} duty cycle");
            assert_eq!(entry.bw_khz, *bw, "{name} max bandwidth");
            assert_eq!(region(entry.name), Some(entry), "{name} name lookup");
        }
        // EU/Norway first; Norway is covered by EU868.
        assert_eq!(REGIONS[0].name, "EU868");
        assert_eq!(region("NOPE"), None);
    }

    #[test]
    fn profile_bandwidth_validation_matrix() {
        use crate::error::Error;
        let cases = [
            // (region, SHORT_TURBO allowed?)
            ("EU868", false),
            ("EU433", false),
            ("US915", true),
            ("AU915", true),
            ("IN865", false),
            ("KR920", false),
            ("AS923", false),
        ];
        for (name, turbo_allowed) in cases {
            let region = region(name).unwrap();
            assert_eq!(
                region.allows_profile(&SHORT_TURBO),
                turbo_allowed,
                "{name} SHORT_TURBO availability"
            );
            if !turbo_allowed {
                assert!(
                    matches!(
                        validate_profile(region, &SHORT_TURBO),
                        Err(Error::Config(_))
                    ),
                    "{name} must reject SHORT_TURBO"
                );
            } else {
                validate_profile(region, &SHORT_TURBO).unwrap();
            }
            // 250 kHz presets are legal everywhere.
            for profile in [&LONG_SLOW, &LONG_FAST, &MEDIUM_FAST, &SHORT_FAST] {
                assert!(
                    region.allows_profile(profile),
                    "{name} rejects {}",
                    profile.name
                );
                assert_eq!(
                    validate_profile(region, profile).is_ok(),
                    profile.bw_khz <= region.bw_khz
                );
            }
            // The fallback profile is always legal, and EU433 prefers LONG_FAST.
            let fallback = region.fallback_profile();
            assert!(
                region.allows_profile(fallback),
                "{name} fallback must be legal"
            );
            let expected_fallback = if name == "EU433" {
                "LONG_FAST"
            } else {
                "MEDIUM_FAST"
            };
            assert_eq!(fallback.name, expected_fallback, "{name} fallback profile");
        }
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
