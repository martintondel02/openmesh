//! Regulatory override: warning-gated, explicitly acknowledged deviations from the legal
//! maxima in [`crate::util::REGIONS`].
//!
//! Normative contract: `docs/design/regulatory-and-regions.md` §2 (v0.5.0). This module is
//! IO-free and dependency-free apart from `core`/`std`: the core engine only carries the
//! override and exposes the effective duty-cycle policy; TX power and centre frequency are
//! applied by the radio layer (bridge config, firmware radio driver).
//!
//! The protocol wire format does not change: the override is local configuration only.
//!
//! > **Not legal advice.** The region numbers are our best reading of the current rules and the
//! > power values are EIRP ceilings; national law changes and antenna gain change effective
//! > radiated power. Use [`OVERRIDE_WARNING`] verbatim on every surface that enables the
//! > override.

use core::fmt;

use crate::util::Region;

/// Warning shown to the user before an override can be enabled or widened.
///
/// This string is normative (docs/design/regulatory-and-regions.md §2.1) and MUST be surfaced
/// verbatim by the bridge API, CLI, MCP, FFI, web and Android clients.
pub const OVERRIDE_WARNING: &str = "Regulatory override active — transmissions may violate local radio regulations and void your radio's certification. Use only in a life-threatening emergency where local law permits. You are responsible for compliance and for any interference or damage.";

/// Maximum length of [`RegulatoryOverride::reason`] in characters.
pub const OVERRIDE_REASON_MAX_CHARS: usize = 120;

/// Lowest centre frequency a SX126x/LR1110 class radio can tune to, in kHz.
pub const RADIO_MIN_FREQUENCY_KHZ: u32 = 150_000;
/// Highest centre frequency a SX126x/LR1110 class radio can tune to, in kHz.
pub const RADIO_MAX_FREQUENCY_KHZ: u32 = 960_000;

/// Local radio-configuration override of the legal duty cycle, TX power and/or frequency.
///
/// `Default` is the safe state: disabled, nothing acknowledged, every field `None` (meaning
/// "use the region default"). `enabled = false` clears the effect but keeps the fields for
/// convenience (`status.override_active = false`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RegulatoryOverride {
    /// Whether the override is applied.
    pub enabled: bool,
    /// Explicit typed acknowledgement of [`OVERRIDE_WARNING`]. Required before the override may
    /// exceed a legal limit.
    pub acknowledged: bool,
    /// Duty cycle in percent, `0..=100`; `None` uses the region duty. `Some(0)` means
    /// "transmit nothing", which is always legal and needs no acknowledgement.
    pub duty_cycle_pct: Option<u8>,
    /// TX power in dBm, clamped to the hardware maximum; `None` uses the region maximum.
    pub max_power_dbm: Option<i8>,
    /// Custom centre frequency in kHz; `None` uses the region centre. The bridge/radio layer
    /// validates the tunable range (see [`frequency_supported`]).
    pub frequency_khz: Option<u32>,
    /// Short free-text reason (at most [`OVERRIDE_REASON_MAX_CHARS`] characters), audited by the
    /// bridge.
    pub reason: Option<String>,
}

impl RegulatoryOverride {
    /// The disabled, no-op override (same as [`RegulatoryOverride::default`]).
    pub fn disabled() -> Self {
        Self::default()
    }

    /// Whether the override is currently applied (`enabled`, regardless of how narrow it is).
    pub fn is_active(&self) -> bool {
        self.enabled
    }

    /// The normative warning to display, or `None` when the override is inactive.
    pub fn warning(&self) -> Option<&'static str> {
        if self.is_active() {
            Some(OVERRIDE_WARNING)
        } else {
            None
        }
    }
}

/// Why a [`RegulatoryOverride`] is invalid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegulatoryOverrideError {
    /// The override exceeds a legal limit but has not been acknowledged.
    RequiresAcknowledgement,
    /// `duty_cycle_pct` is greater than 100.
    DutyCycleOutOfRange,
    /// `reason` is longer than [`OVERRIDE_REASON_MAX_CHARS`] characters.
    ReasonTooLong,
}

impl RegulatoryOverrideError {
    /// Stable machine-readable code for API error mapping (snake_case, additive).
    pub const fn code(self) -> &'static str {
        match self {
            Self::RequiresAcknowledgement => "regulatory_override_requires_acknowledgement",
            Self::DutyCycleOutOfRange => "regulatory_override_duty_out_of_range",
            Self::ReasonTooLong => "regulatory_override_reason_too_long",
        }
    }

    /// Human-readable message (the acknowledgement message matches the normative contract).
    pub const fn message(self) -> &'static str {
        match self {
            Self::RequiresAcknowledgement => "regulatory override requires acknowledgement",
            Self::DutyCycleOutOfRange => "regulatory override duty cycle must be 0..=100 percent",
            Self::ReasonTooLong => "regulatory override reason must be at most 120 characters",
        }
    }
}

impl fmt::Display for RegulatoryOverrideError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for RegulatoryOverrideError {}

/// Validate an override against a region's legal limits.
///
/// Rules (docs/design/regulatory-and-regions.md §2.1):
/// 1. `enabled` may only exceed a legal limit when `acknowledged` is set.
/// 2. `duty_cycle_pct` must be `0..=100`; `Some(0)` ("no transmissions") is always legal and
///    needs no acknowledgement.
/// 3. Acknowledgement is required only when the override **exceeds** the legal limit
///    (higher duty, higher power, or a frequency different from the region centre). Lowering
///    power or duty, or staying on the region frequency, needs no acknowledgement.
/// 4. `reason` must be at most [`OVERRIDE_REASON_MAX_CHARS`] characters.
///
/// A disabled override is always valid apart from the plain range checks above, so users can
/// keep their values while it is turned off.
pub fn validate(region: &Region, over: &RegulatoryOverride) -> Result<(), RegulatoryOverrideError> {
    if let Some(reason) = &over.reason {
        if reason.chars().count() > OVERRIDE_REASON_MAX_CHARS {
            return Err(RegulatoryOverrideError::ReasonTooLong);
        }
    }
    if let Some(duty) = over.duty_cycle_pct {
        if duty > 100 {
            return Err(RegulatoryOverrideError::DutyCycleOutOfRange);
        }
    }
    if over.enabled && !over.acknowledged && exceeds_legal(region, over) {
        return Err(RegulatoryOverrideError::RequiresAcknowledgement);
    }
    Ok(())
}

/// Whether an **enabled** override exceeds any legal limit of `region`.
///
/// A disabled override never exceeds anything. Duty comparison uses 100 % for regions with no
/// statutory duty cycle (fair use), so any valid 0..=100 setting is not "exceeding" there.
pub fn exceeds_legal(region: &Region, over: &RegulatoryOverride) -> bool {
    if !over.enabled {
        return false;
    }
    let duty_exceeds = match over.duty_cycle_pct {
        Some(duty) => duty > region.duty_cycle_pct.unwrap_or(100),
        None => false,
    };
    let power_exceeds = match over.max_power_dbm {
        Some(power) => power > region.max_power_dbm,
        None => false,
    };
    let frequency_exceeds = match over.frequency_khz {
        Some(frequency) => frequency != region.freq_khz,
        None => false,
    };
    duty_exceeds || power_exceeds || frequency_exceeds
}

/// Effective duty-cycle budget in percent, or `None` for regions with only fair-use rules.
pub fn effective_duty_pct(region: &Region, over: &RegulatoryOverride) -> Option<u8> {
    if over.enabled {
        over.duty_cycle_pct.or(region.duty_cycle_pct)
    } else {
        region.duty_cycle_pct
    }
}

/// Effective TX power in dBm, never above the hardware cap (contract §1: the board cap always
/// wins, even under an override).
pub fn effective_power_dbm(region: &Region, over: &RegulatoryOverride, hardware_max_dbm: i8) -> i8 {
    let requested = if over.enabled {
        over.max_power_dbm.unwrap_or(region.max_power_dbm)
    } else {
        region.max_power_dbm
    };
    requested.min(hardware_max_dbm)
}

/// Effective centre frequency in kHz (region centre unless an active override sets one).
pub fn effective_frequency_khz(region: &Region, over: &RegulatoryOverride) -> u32 {
    if over.enabled {
        over.frequency_khz.unwrap_or(region.freq_khz)
    } else {
        region.freq_khz
    }
}

/// Whether the radio can tune to `frequency_khz` (SX126x/LR1110 ≈ 150–960 MHz).
///
/// The bridge MUST reject out-of-range values with HTTP 422 rather than silently ignoring them;
/// this helper exists so host and client agree on the range.
pub fn frequency_supported(frequency_khz: u32) -> bool {
    (RADIO_MIN_FREQUENCY_KHZ..=RADIO_MAX_FREQUENCY_KHZ).contains(&frequency_khz)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::{Region, region};

    fn eu868() -> &'static Region {
        region("EU868").expect("EU868 region")
    }

    fn us915() -> &'static Region {
        region("US915").expect("US915 region")
    }

    fn acknowledged(fields: RegulatoryOverride) -> RegulatoryOverride {
        RegulatoryOverride {
            acknowledged: true,
            ..fields
        }
    }

    #[test]
    fn default_is_disabled_and_safe() {
        let over = RegulatoryOverride::default();
        assert!(!over.is_active());
        assert!(!over.enabled && !over.acknowledged);
        assert_eq!(over.duty_cycle_pct, None);
        assert_eq!(over.max_power_dbm, None);
        assert_eq!(over.frequency_khz, None);
        assert_eq!(over.reason, None);
        assert!(validate(eu868(), &over).is_ok());
        assert!(!exceeds_legal(eu868(), &over));
        assert_eq!(effective_duty_pct(eu868(), &over), Some(10));
        assert_eq!(effective_power_dbm(eu868(), &over, 22), 22);
        assert_eq!(effective_frequency_khz(eu868(), &over), 869_525);
        assert_eq!(over.warning(), None);
    }

    #[test]
    fn legal_maxima_are_the_region_defaults() {
        let over = RegulatoryOverride::default();
        assert_eq!(effective_duty_pct(us915(), &over), None);
        assert_eq!(effective_power_dbm(us915(), &over, 30), 30);
        assert_eq!(effective_power_dbm(us915(), &over, 22), 22);
        assert_eq!(effective_frequency_khz(us915(), &over), 915_000);
    }

    #[test]
    fn enabled_requires_acknowledgement_only_when_exceeding() {
        // Exceeding power without acknowledgement fails.
        let over = RegulatoryOverride {
            enabled: true,
            max_power_dbm: Some(30),
            ..Default::default()
        };
        assert_eq!(
            validate(eu868(), &over),
            Err(RegulatoryOverrideError::RequiresAcknowledgement)
        );
        assert_eq!(
            RegulatoryOverrideError::RequiresAcknowledgement.code(),
            "regulatory_override_requires_acknowledgement"
        );
        assert_eq!(
            RegulatoryOverrideError::RequiresAcknowledgement.to_string(),
            "regulatory override requires acknowledgement"
        );

        // Acknowledged is accepted.
        let over = acknowledged(over);
        assert!(validate(eu868(), &over).is_ok());
        assert!(exceeds_legal(eu868(), &over));

        // Safe reductions never need acknowledgement.
        for over in [
            RegulatoryOverride {
                enabled: true,
                duty_cycle_pct: Some(5),
                ..Default::default()
            },
            RegulatoryOverride {
                enabled: true,
                max_power_dbm: Some(10),
                ..Default::default()
            },
            RegulatoryOverride {
                enabled: true,
                duty_cycle_pct: Some(10),
                max_power_dbm: Some(27),
                frequency_khz: Some(869_525),
                ..Default::default()
            },
            RegulatoryOverride {
                enabled: true,
                duty_cycle_pct: Some(0),
                ..Default::default()
            },
        ] {
            assert!(
                !exceeds_legal(eu868(), &over),
                "unexpected exceed: {over:?}"
            );
            assert!(
                validate(eu868(), &over).is_ok(),
                "safe override rejected: {over:?}"
            );
        }
    }

    #[test]
    fn duty_cycle_bounds() {
        let over = acknowledged(RegulatoryOverride {
            enabled: true,
            duty_cycle_pct: Some(101),
            ..Default::default()
        });
        assert_eq!(
            validate(eu868(), &over),
            Err(RegulatoryOverrideError::DutyCycleOutOfRange)
        );
        assert_eq!(over.duty_cycle_pct, Some(101));
        assert_eq!(
            RegulatoryOverrideError::DutyCycleOutOfRange.message(),
            "regulatory override duty cycle must be 0..=100 percent"
        );
        // 100 % is valid on a duty-cycle region but needs the acknowledgement.
        let over = acknowledged(RegulatoryOverride {
            enabled: true,
            duty_cycle_pct: Some(100),
            ..Default::default()
        });
        assert!(validate(eu868(), &over).is_ok());
        assert!(exceeds_legal(eu868(), &over));
        // 0 % means "no transmissions": legal, no acknowledgement needed.
        let over = RegulatoryOverride {
            enabled: true,
            duty_cycle_pct: Some(0),
            ..Default::default()
        };
        assert!(validate(eu868(), &over).is_ok());
    }

    #[test]
    fn reason_length_is_capped() {
        let long = "x".repeat(OVERRIDE_REASON_MAX_CHARS + 1);
        let over = RegulatoryOverride {
            enabled: false,
            reason: Some(long.clone()),
            ..Default::default()
        };
        assert_eq!(
            validate(eu868(), &over),
            Err(RegulatoryOverrideError::ReasonTooLong)
        );
        // Exactly the limit is accepted; a disabled override keeps fields without ack.
        let at_limit = "y".repeat(OVERRIDE_REASON_MAX_CHARS);
        let over = RegulatoryOverride {
            enabled: false,
            reason: Some(at_limit),
            duty_cycle_pct: Some(100),
            max_power_dbm: Some(30),
            ..Default::default()
        };
        assert!(validate(eu868(), &over).is_ok());
        // Character count, not byte count: 120 multi-byte characters are fine.
        let unicode = "ø".repeat(OVERRIDE_REASON_MAX_CHARS);
        let over = RegulatoryOverride {
            reason: Some(unicode),
            ..Default::default()
        };
        assert!(validate(eu868(), &over).is_ok());
    }

    #[test]
    fn effective_policy_matrix() {
        // Region defaults when inactive.
        let off = RegulatoryOverride::default();
        assert_eq!(effective_duty_pct(eu868(), &off), Some(10));
        assert_eq!(effective_power_dbm(eu868(), &off, 22), 22);
        assert_eq!(effective_frequency_khz(eu868(), &off), 869_525);

        // Override fields replace region defaults when active.
        let on = acknowledged(RegulatoryOverride {
            enabled: true,
            duty_cycle_pct: Some(100),
            max_power_dbm: Some(22),
            frequency_khz: Some(868_000),
            ..Default::default()
        });
        assert_eq!(effective_duty_pct(eu868(), &on), Some(100));
        assert_eq!(effective_power_dbm(eu868(), &on, 30), 22);
        // Hardware cap wins even when the override asks for more.
        assert_eq!(effective_power_dbm(eu868(), &on, 15), 15);
        assert_eq!(effective_frequency_khz(eu868(), &on), 868_000);

        // None fields fall back to region values; fair-use region stays None.
        let partial = acknowledged(RegulatoryOverride {
            enabled: true,
            ..Default::default()
        });
        assert_eq!(effective_duty_pct(us915(), &partial), None);
        assert_eq!(effective_duty_pct(eu868(), &partial), Some(10));
        assert_eq!(effective_power_dbm(us915(), &partial, 22), 22);
        assert_eq!(effective_frequency_khz(us915(), &partial), 915_000);

        // Disabling keeps the fields but ignores them.
        let disabled = RegulatoryOverride {
            enabled: false,
            acknowledged: true,
            duty_cycle_pct: Some(100),
            max_power_dbm: Some(30),
            frequency_khz: Some(868_000),
            ..Default::default()
        };
        assert!(!disabled.is_active());
        assert_eq!(effective_duty_pct(eu868(), &disabled), Some(10));
        assert_eq!(effective_power_dbm(eu868(), &disabled, 22), 22);
        assert_eq!(effective_frequency_khz(eu868(), &disabled), 869_525);
        assert_eq!(disabled.warning(), None);
    }

    #[test]
    fn frequency_range_helper() {
        assert!(frequency_supported(869_525));
        assert!(frequency_supported(RADIO_MIN_FREQUENCY_KHZ));
        assert!(frequency_supported(RADIO_MAX_FREQUENCY_KHZ));
        assert!(!frequency_supported(RADIO_MIN_FREQUENCY_KHZ - 1));
        assert!(!frequency_supported(RADIO_MAX_FREQUENCY_KHZ + 1));
        assert!(!frequency_supported(100_000));
        // A custom in-range frequency still counts as "exceeding" for acknowledgement purposes.
        let over = acknowledged(RegulatoryOverride {
            enabled: true,
            frequency_khz: Some(868_000),
            ..Default::default()
        });
        assert!(exceeds_legal(eu868(), &over));
    }

    #[test]
    fn warning_is_normative_text() {
        let active = acknowledged(RegulatoryOverride {
            enabled: true,
            ..Default::default()
        });
        assert_eq!(active.warning(), Some(OVERRIDE_WARNING));
        assert!(OVERRIDE_WARNING.contains("Regulatory override active"));
        assert!(OVERRIDE_WARNING.contains("You are responsible for compliance"));
        assert!(OVERRIDE_WARNING.contains("life-threatening emergency"));
    }
}
