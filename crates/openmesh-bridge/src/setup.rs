//! First-run checklist, region/role recommendations and the secure beginner preset.
//!
//! Normative sources: `docs/design/ux-help-system.md` §3/§5, `docs/architecture/AI-NATIVE.md`
//! (MCP `openmesh_setup_state` / `openmesh_setup_quick`) and `docs/design/android-ga.md` §3.
//!
//! The country→region table and the four role presets are **shared data**, not code: they live in
//! `content/presets/setup.json`, which is embedded here at build time (and shipped by the Android
//! app and the web demo). The file is validated when the bridge starts, so a malformed preset
//! table fails fast instead of silently changing onboarding behaviour.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::{Map, Value, json};

use openmesh_core::util;

use crate::json::{ChannelCreateData, ConfigJson, b64_decode, parse_node_id};
use crate::runtime::{self, CommandError, CommandKind, Shared};
use crate::transport::backbone::BackboneManager;

/// Shared preset file, embedded from the repository root at compile time.
pub const PRESETS_JSON: &str = include_str!("../../../content/presets/setup.json");

/// Canonical role order used by the wizard/CLI/Android onboarding.
pub const ROLE_ORDER: &[&str] = &["client", "tracker", "repeater", "gateway"];

#[derive(Clone, Debug, Deserialize)]
pub struct RolePresetJson {
    /// LoRa profile name.
    pub profile: String,
    /// Whether the role stores traffic for offline peers.
    pub store_forward: bool,
    /// Position policy: `private` (manual/direct only), `channel` (shared to channels) or `off`.
    pub position_sharing: String,
    /// Announce period in seconds.
    pub announce_interval_s: u64,
    /// Beginner-facing explanation of the role.
    #[serde(default)]
    pub description: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SetupPresets {
    /// Preset file format version.
    pub version: u32,
    /// Human description of the file.
    #[allow(dead_code)]
    pub description: String,
    /// Region used when the country is unknown.
    pub default_region: String,
    /// ISO 3166-1 alpha-2 country → regulatory region.
    pub country_regions: BTreeMap<String, String>,
    /// Role presets (client, tracker, repeater, gateway).
    pub roles: BTreeMap<String, RolePresetJson>,
    /// Informational secure-defaults block (consumed by clients from the same JSON file).
    #[allow(dead_code)]
    pub security: Value,
    /// Informational media notes (consumed by clients from the same JSON file).
    #[allow(dead_code)]
    pub media: Value,
}

static PRESETS: OnceLock<SetupPresets> = OnceLock::new();

/// Parse and validate a preset document (the embedded file, or a test fixture).
pub fn presets_from_json(text: &str) -> Result<SetupPresets, String> {
    let presets: SetupPresets =
        serde_json::from_str(text).map_err(|e| format!("setup presets are not valid JSON: {e}"))?;
    validate_presets(&presets)?;
    Ok(presets)
}

fn valid_region(name: &str) -> bool {
    util::REGIONS.iter().any(|region| region.name == name)
}

fn valid_profile(name: &str) -> bool {
    util::PROFILES.iter().any(|profile| profile.name == name)
}

fn validate_presets(presets: &SetupPresets) -> Result<(), String> {
    if presets.version < 1 {
        return Err(format!(
            "setup presets version {} is not supported",
            presets.version
        ));
    }
    if !valid_region(&presets.default_region) {
        return Err(format!(
            "setup presets default_region '{}' is not a known region",
            presets.default_region
        ));
    }
    if presets.country_regions.is_empty() {
        return Err("setup presets country_regions is empty".to_string());
    }
    for (country, region) in &presets.country_regions {
        if country.len() != 2 || !country.chars().all(|c| c.is_ascii_alphabetic()) {
            return Err(format!(
                "setup presets country key '{country}' is not an ISO 3166-1 alpha-2 code"
            ));
        }
        if !valid_region(region) {
            return Err(format!(
                "setup presets maps {country} to unknown region '{region}'"
            ));
        }
    }
    for required in ROLE_ORDER {
        let role = presets
            .roles
            .get(*required)
            .ok_or_else(|| format!("setup presets are missing required role '{required}'"))?;
        if !valid_profile(&role.profile) {
            return Err(format!(
                "setup presets role '{required}' uses unknown profile '{}'",
                role.profile
            ));
        }
        if !matches!(
            role.position_sharing.as_str(),
            "private" | "channel" | "off"
        ) {
            return Err(format!(
                "setup presets role '{required}' has unknown position_sharing '{}' (expected private, channel or off)",
                role.position_sharing
            ));
        }
        if !(300..=24 * 3600).contains(&role.announce_interval_s) {
            return Err(format!(
                "setup presets role '{required}' announce_interval_s {} is outside 300..=86400",
                role.announce_interval_s
            ));
        }
    }
    Ok(())
}

/// Validate the embedded preset file. Called once during bridge startup so a corrupt embed fails
/// fast; also used by the tests.
pub fn validate_embedded_presets() -> Result<(), String> {
    presets_from_json(PRESETS_JSON).map(|_| ())
}

/// The validated embedded presets.
pub fn presets() -> &'static SetupPresets {
    PRESETS.get_or_init(|| {
        presets_from_json(PRESETS_JSON)
            .expect("embedded content/presets/setup.json is valid (validated at bridge startup)")
    })
}

/// Role names in canonical order.
pub fn roles() -> Vec<&'static str> {
    ROLE_ORDER.to_vec()
}

/// Server-owned preset for one role (UX spec §5 table / shared setup.json).
#[derive(Clone, Copy, Debug)]
pub struct RolePreset {
    pub role: &'static str,
    pub profile: &'static str,
    pub store_forward: bool,
    pub position_sharing: bool,
    pub announce_interval_s: u64,
}

fn static_role(role: &str) -> (&'static str, &'static RolePresetJson) {
    let presets = presets();
    let key = presets
        .roles
        .keys()
        .find(|name| name.eq_ignore_ascii_case(role))
        .map(String::as_str)
        .unwrap_or("client");
    let preset = presets
        .roles
        .get(key)
        .expect("role 'client' exists in validated setup presets");
    (key, preset)
}

pub fn role_preset(role: &str) -> RolePreset {
    let (name, preset) = static_role(role);
    RolePreset {
        role: name,
        profile: preset.profile.as_str(),
        store_forward: preset.store_forward,
        // Policy → legacy boolean used by `/api/setup/*`: only the tracker's "channel" policy
        // enables position sharing by default; "private"/"off" start disabled.
        position_sharing: preset.position_sharing == "channel",
        announce_interval_s: preset.announce_interval_s,
    }
}

/// Whether a role policy enables position sharing in the config feature flag.
fn position_policy_enabled(role: &str) -> bool {
    let (_, preset) = static_role(role);
    preset.position_sharing == "channel"
}

/// Map a country code to a regulatory region (shared `country_regions` table).
///
/// `EU` is accepted as an alias for the default region and `UK` for `GB`; unknown codes fall back
/// to the default with a visible warning.
pub fn country_region(country: &str) -> (&'static str, Option<String>) {
    let presets = presets();
    let code = country.trim().to_ascii_uppercase();
    if code == "EU" {
        return (presets.default_region.as_str(), None);
    }
    let code = if code == "UK" { "GB" } else { code.as_str() };
    match presets.country_regions.get(code) {
        Some(region) => (region.as_str(), None),
        None => (
            presets.default_region.as_str(),
            Some(format!(
                "Unknown country '{code}'; defaulting to {}. Pick your region in Settings if that is wrong.",
                presets.default_region
            )),
        ),
    }
}

fn infer_role(class: &str) -> &'static str {
    match class {
        "repeater" => "repeater",
        "gateway" | "room_server" => "gateway",
        "tracker" => "tracker",
        _ => "client",
    }
}

fn normalize_role(role: Option<&str>, current: &ConfigJson) -> &'static str {
    if let Some(role) = role {
        let role = role.trim().to_ascii_lowercase();
        if let Some((key, _)) = presets()
            .roles
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(&role))
        {
            return key.as_str();
        }
    }
    infer_role(&current.node.class)
}

fn region_info(region: &str) -> Option<&'static util::Region> {
    util::REGIONS.iter().find(|info| info.name == region)
}

fn role_note(role: &str) -> String {
    let (_, preset) = static_role(role);
    preset.description.clone()
}

/// `GET /api/setup/recommend` payload (also embedded in setup state and quick setup).
pub fn recommend(country: Option<&str>, role: Option<&str>, current: &ConfigJson) -> Value {
    let role = normalize_role(role, current);
    let preset = role_preset(role);
    let (region, warning): (String, Option<String>) =
        match country.map(str::trim).filter(|c| !c.is_empty()) {
            Some(country) => {
                let (region, warning) = country_region(country);
                (region.to_string(), warning)
            }
            None => (current.radio.region.clone(), None),
        };
    let info = region_info(&region);
    let mut notes = vec![
        format!(
            "{} uses {} and announces every {} minutes.",
            preset.role,
            preset.profile,
            preset.announce_interval_s / 60
        ),
        if preset.store_forward {
            "Store-and-forward is on: this node holds messages for people who are offline."
                .to_string()
        } else {
            "Store-and-forward is off: this node keeps its battery for its own traffic.".to_string()
        },
        if preset.position_sharing {
            "Position sharing is on for the tracker preset; share only with a channel you trust."
                .to_string()
        } else {
            "Position sharing is off by default; you can turn it on per contact or channel."
                .to_string()
        },
    ];
    let description = role_note(role);
    if !description.is_empty() {
        notes.push(description);
    }
    if let Some(info) = info {
        if let Some(duty) = info.duty_cycle_pct {
            notes.push(format!(
                "{} allows a {}% duty cycle; keep repeaters well under it.",
                info.name, duty
            ));
        } else {
            notes.push(format!(
                "{} uses fair-use rules rather than a duty cycle.",
                info.name
            ));
        }
    }
    let legal = region_info(&region)
        .map(crate::json::LegalRegionJson::for_region)
        .and_then(|legal| serde_json::to_value(legal).ok())
        .unwrap_or(Value::Null);
    json!({
        "country": country,
        "role": role,
        "region": region,
        "profile": preset.profile,
        "legal": legal,
        "store_forward": preset.store_forward,
        "position_sharing": preset.position_sharing,
        "announce_interval_s": preset.announce_interval_s,
        "duty_cycle_pct": info.and_then(|info| info.duty_cycle_pct),
        "frequency_mhz": info.map(|info| info.freq_khz as f64 / 1000.0),
        "notes": notes,
        "warning": warning,
    })
}

/// Public-exposure guidance for `/api/backbone/setup` (BACKBONE.md §8/§9b).
///
/// The web panel and CLI render this verbatim (copy buttons + docs link); the object contains no
/// secrets — only the recommended provider, when to use it, the loopback bind command and the
/// alternatives. `port` is the live OBB/1 port.
pub fn exposure_payload(port: u16) -> Value {
    json!({
        "recommended": "openflare",
        "provider": {
            "name": "Openflare",
            "url": "https://github.com/martintondel02/openflare",
            "description": "Self-hosted tunnel, reverse proxy and control plane with outbound-only connectors and one public edge."
        },
        "when_to_use": "Your backbone is behind NAT/CGNAT, or you do not want to open an inbound port.",
        "steps": [
            "bind the backbone to 127.0.0.1",
            format!("create a TCP tunnel to 127.0.0.1:{port}"),
            "set public_host to the tunnel hostname",
            "share the connection string"
        ],
        "alternatives": ["WireGuard", "Tailscale", "Cloudflare Tunnel", "Caddy/nginx stream TLS"],
        "bind_command": format!("openmesh-bridge backbone --bind 127.0.0.1 --port {port} --public-host <host>"),
        "guide_url": "https://github.com/martintondel02/openmesh/blob/main/docs/guides/backbone-exposure.md"
    })
}

/// The onboarding step that follows "Open the firewall" (BACKBONE.md §8/§9b).
pub fn exposure_step(port: u16) -> Value {
    json!({
        "title": "Expose it publicly (optional)",
        "commands": [
            format!("openmesh-bridge backbone --bind 127.0.0.1 --port {port} --public-host <tunnel-host>"),
            format!("# create an outbound-only TCP tunnel to 127.0.0.1:{port} (Openflare connector, WireGuard or Tailscale)"),
            "openmesh-bridge join '<connection-string>'   # verify from another network"
        ],
        "note": "Skip if gateways already share a VPN. Expose only the OBB/1 port; keep the API panel loopback-only, and never switch auth to open."
    })
}

/// `GET /api/setup/state`: first-run flag, checklist, recommendations and counts.
pub async fn state(
    shared: &Shared,
    backbone: &BackboneManager,
    activity: &crate::activity::ActivityLog,
    has_api_token: bool,
) -> Value {
    let snapshot = shared.snapshot.read().await.clone();
    let completed: Option<bool> = shared
        .store
        .get_meta_json("setup_completed")
        .unwrap_or(None);
    let setup_completed = completed.unwrap_or(false);
    let backbone_desc = backbone.describe().await;
    let backbone_configured = backbone_desc.get("bind").is_some();
    let radios_connected = !snapshot.radios.is_empty();
    let channel_count = snapshot.store.channels;
    let recommended = recommend(None, None, &snapshot.config);
    let override_json = snapshot.config.regulatory_override.clone();
    let override_active = override_json.enabled;
    let checks = json!({
        "identity": {
            "ok": !snapshot.node_id.is_empty(),
            "detail": format!("node id {}", snapshot.node_id),
        },
        "radio": {
            "ok": radios_connected,
            "detail": if radios_connected {
                format!("{} transport(s) configured", snapshot.radios.len())
            } else {
                "no radio transport detected yet".to_string()
            },
        },
        "api_token": {
            "ok": has_api_token,
            "detail": if has_api_token { "the API requires a bearer token" } else { "no API token configured" },
        },
        "channel": {
            "ok": channel_count > 0,
            "count": channel_count,
            "detail": if channel_count > 0 { "a private channel is ready to invite from" } else { "create a channel to invite someone" },
        },
        "backbone": {
            "ok": backbone_configured,
            "mode": backbone_desc.get("mode").cloned().unwrap_or(Value::String("disabled".into())),
            "detail": if backbone_configured { "backbone mode is configured" } else { "run a backbone or connect a gateway" },
        },
        "agent": {
            "ok": activity.has_agent_activity(),
            "last_action_ms": (activity.last_agent_at_ms() > 0).then(|| activity.last_agent_at_ms()),
            "detail": if activity.has_agent_activity() { "an AI agent has acted on this bridge" } else { "connect an AI agent with the MCP server" },
        },
    });
    json!({
        "node_id": snapshot.node_id,
        "node_name": snapshot.config.node.name,
        "first_run": !setup_completed,
        "setup_completed": setup_completed,
        "checks": checks,
        "recommended": {
            "region": recommended["region"],
            "profile": recommended["profile"],
            "role": recommended["role"],
            "legal": recommended["legal"],
        },
        "regulatory_override": {
            "enabled": override_json.enabled,
            "acknowledged": override_json.acknowledged,
        },
        "override_active": override_active,
        "override_warning": override_json.warning(),
        "counts": {
            "nodes": snapshot.nodes.len(),
            "messages": snapshot.store.messages,
            "contacts": snapshot.store.contacts,
            "channels": channel_count,
            "radios": snapshot.radios.len(),
            "backbone_peers": backbone_desc.get("peers").cloned().unwrap_or(json!(0)),
        },
    })
}

fn bad_request(message: impl Into<String>) -> CommandError {
    CommandError::new("bad_request", message)
}

fn next_steps() -> Vec<Value> {
    vec![
        json!({ "title": "Send your first message", "detail": "Open Chats and write to a channel or a node you can see.", "help_topic": "messaging" }),
        json!({ "title": "Invite someone", "detail": "Share the channel invite link or QR code from the channel page.", "help_topic": "channels" }),
        json!({ "title": "Add a contact", "detail": "Scan a contact card so you can send private messages later.", "help_topic": "contacts-identity" }),
        json!({ "title": "Run a backbone or connect an agent", "detail": "Backbone links meshes over the internet; the MCP server lets an AI agent operate this node.", "help_topic": "backbone" }),
    ]
}

/// `POST /api/setup/quick`: apply the role/country preset once, idempotently.
pub async fn quick(shared: &Shared, body: &Value) -> Result<Value, CommandError> {
    let object = body
        .as_object()
        .ok_or_else(|| bad_request("setup body must be a JSON object"))?;
    let role = object
        .get("role")
        .and_then(|v| v.as_str())
        .unwrap_or("client")
        .trim()
        .to_ascii_lowercase();
    if !presets().roles.contains_key(&role) {
        return Err(bad_request(format!(
            "role must be one of {}",
            roles().join(", ")
        )));
    }
    let country = object
        .get("country")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|country| !country.is_empty())
        .ok_or_else(|| bad_request("country is required (ISO code such as NO)"))?;
    if country.len() > 32 {
        return Err(bad_request("country code is too long"));
    }
    let privacy = object
        .get("privacy")
        .and_then(|v| v.as_str())
        .unwrap_or("private")
        .to_ascii_lowercase();
    if privacy != "private" && privacy != "shared" {
        return Err(bad_request("privacy must be private or shared"));
    }
    let create_channel = object
        .get("create_channel")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let node_name = object
        .get("node_name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| name.chars().take(32).collect::<String>());

    let completed: Option<bool> = shared
        .store
        .get_meta_json("setup_completed")
        .unwrap_or(None);
    let first_run = !completed.unwrap_or(false);
    let recommended = {
        let snapshot = shared.snapshot.read().await;
        recommend(Some(country), Some(&role), &snapshot.config)
    };

    let mut patch = Map::new();
    let mut applied = Map::new();
    {
        let snapshot = shared.snapshot.read().await;
        let mut node_patch = Map::new();
        if let Some(name) = &node_name {
            if snapshot.config.node.name != *name {
                node_patch.insert("name".into(), json!(name));
                applied.insert("node_name".into(), json!(name));
            }
        }
        if snapshot.config.node.class != role {
            node_patch.insert("class".into(), json!(role));
            applied.insert("role".into(), json!(role));
        }
        if !node_patch.is_empty() {
            patch.insert("node".into(), Value::Object(node_patch));
        }
        let mut radio_patch = Map::new();
        let region = recommended["region"].as_str().unwrap_or("EU868");
        if snapshot.config.radio.region != region {
            radio_patch.insert("region".into(), json!(region));
            applied.insert("region".into(), json!(region));
        }
        let profile = recommended["profile"].as_str().unwrap_or("MEDIUM_FAST");
        if snapshot.config.radio.profile != profile {
            radio_patch.insert("profile".into(), json!(profile));
            applied.insert("profile".into(), json!(profile));
        }
        if !radio_patch.is_empty() {
            patch.insert("radio".into(), Value::Object(radio_patch));
        }
        let mut features = Map::new();
        let store_forward = recommended["store_forward"].as_bool().unwrap_or(false);
        if snapshot.config.features.store_forward != store_forward {
            features.insert("store_forward".into(), json!(store_forward));
            applied.insert("store_forward".into(), json!(store_forward));
        }
        // Position: the shared preset decides role policy ("channel" → on); a client can opt in
        // with privacy=shared. Repeaters/gateways keep it off.
        let position_sharing =
            position_policy_enabled(&role) || (role == "client" && privacy == "shared");
        if snapshot.config.features.position_sharing != position_sharing {
            features.insert("position_sharing".into(), json!(position_sharing));
            applied.insert("position_sharing".into(), json!(position_sharing));
        }
        if !snapshot.config.features.auto_announce {
            features.insert("auto_announce".into(), json!(true));
            applied.insert("auto_announce".into(), json!(true));
        }
        if !features.is_empty() {
            patch.insert("features".into(), Value::Object(features));
        }
    }

    if !patch.is_empty() {
        runtime::run_command(shared, CommandKind::SetConfig(Value::Object(patch))).await?;
    }
    if first_run || !applied.is_empty() {
        runtime::queue_command(shared, CommandKind::Announce).await?;
        applied.insert("announced".into(), json!(true));
    }

    let mut channel_value = Value::Null;
    let mut channel_created = false;
    if create_channel {
        let channels = shared
            .store
            .list_channels()
            .map_err(|e| CommandError::new("internal", e.to_string()))?;
        if let Some(existing) = channels
            .iter()
            .find(|channel| channel.name.eq_ignore_ascii_case("general"))
        {
            let invite = existing
                .psk_bytes()
                .map(|psk| runtime::channel_url(existing.channel_id, &existing.name, &psk));
            channel_value = json!({
                "channel_id": existing.channel_id.to_hex(),
                "name": existing.name,
                "invite_url": invite,
                "psk_once": null,
            });
        } else {
            let created = runtime::run_command(
                shared,
                CommandKind::CreateChannel(ChannelCreateData {
                    name: "general".to_string(),
                    psk: None,
                }),
            )
            .await?;
            let channel_id = created["channel_id"]
                .as_str()
                .ok_or_else(|| CommandError::new("internal", "channel reply missing channel_id"))?;
            let psk_once = created["psk"]
                .as_str()
                .ok_or_else(|| CommandError::new("internal", "channel reply missing psk"))?;
            let name = created["name"].as_str().unwrap_or("general").to_string();
            let invite_url = parse_node_id(channel_id)
                .zip(b64_decode(psk_once).and_then(|raw| <[u8; 32]>::try_from(raw).ok()))
                .map(|(id, psk)| runtime::channel_url(id, &name, &psk));
            channel_value = json!({
                "channel_id": channel_id,
                "name": name,
                "invite_url": invite_url,
                "psk_once": psk_once,
            });
            channel_created = true;
        }
    }
    if channel_created {
        applied.insert("channel_created".into(), json!(true));
    }

    shared
        .store
        .set_meta_json("setup_completed", &true)
        .map_err(|e| CommandError::new("internal", e.to_string()))?;

    let snapshot = shared.snapshot.read().await.clone();
    let channels = shared.store.list_channels().unwrap_or_default();
    let checklist = vec![
        json!({ "id": "identity", "label": "Identity created", "ok": !snapshot.node_id.is_empty() }),
        json!({ "id": "radio", "label": "Radio detected", "ok": !snapshot.radios.is_empty() }),
        json!({ "id": "region", "label": "Region set", "ok": snapshot.config.radio.region == recommended["region"] }),
        json!({ "id": "channel", "label": "Private channel created", "ok": !channels.is_empty() }),
        json!({ "id": "secure", "label": "Secure defaults applied", "ok": true }),
    ];
    Ok(json!({
        "applied": Value::Object(applied),
        "channel": channel_value,
        "checklist": checklist,
        "next_steps": next_steps(),
        "recommended": recommended,
        "first_run": false,
        "setup_completed": true,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> ConfigJson {
        ConfigJson {
            node: crate::json::ConfigNodeJson {
                name: "openmesh-bridge".into(),
                class: "client".into(),
                announce_interval_s: 1800,
            },
            ..ConfigJson::default()
        }
    }

    #[test]
    fn embedded_presets_parse_validate_and_cover_ux_spec() {
        validate_embedded_presets().expect("embedded content/presets/setup.json is valid");
        let presets = presets();
        assert_eq!(presets.version, 1);
        assert!(presets.country_regions.len() >= 40, "shared country table");
        // The UX spec's canonical mappings, from the shared file.
        assert_eq!(country_region("NO").0, "EU868");
        assert_eq!(country_region("us").0, "US915");
        assert_eq!(country_region("AU").0, "AU915");
        assert_eq!(country_region("IN").0, "IN865");
        assert_eq!(country_region("KR").0, "KR920");
        assert_eq!(country_region("SG").0, "AS923");
        assert_eq!(country_region("BR").0, "US915");
        // Aliases and unknown countries.
        assert_eq!(country_region("UK").0, "EU868");
        assert_eq!(country_region("EU").0, presets.default_region);
        let (region, warning) = country_region("ZZ");
        assert_eq!(region, presets.default_region);
        let warning = warning.expect("unknown country warning");
        assert!(warning.contains("Unknown country 'ZZ'"));
        assert!(warning.contains(&presets.default_region));
    }

    #[test]
    fn role_presets_match_the_shared_file() {
        let roles = roles();
        assert_eq!(roles, vec!["client", "tracker", "repeater", "gateway"]);
        let client = role_preset("client");
        assert_eq!(client.profile, "MEDIUM_FAST");
        assert!(!client.store_forward);
        assert!(!client.position_sharing);
        assert_eq!(client.announce_interval_s, 1800);
        let tracker = role_preset("tracker");
        assert_eq!(tracker.profile, "MEDIUM_FAST");
        assert!(!tracker.store_forward);
        assert!(tracker.position_sharing, "tracker shares to its channel");
        assert_eq!(tracker.announce_interval_s, 3600);
        let repeater = role_preset("repeater");
        assert_eq!(repeater.profile, "LONG_FAST");
        assert!(repeater.store_forward);
        assert!(!repeater.position_sharing);
        let gateway = role_preset("gateway");
        assert_eq!(gateway.profile, "LONG_FAST");
        assert!(gateway.store_forward);
        // Unknown roles fall back to the client preset (never an empty config).
        assert_eq!(role_preset("wizard").role, "client");
        assert!(position_policy_enabled("tracker"));
        assert!(!position_policy_enabled("client"));
    }

    #[test]
    fn invalid_preset_documents_are_rejected() {
        let base: Value = serde_json::from_str(PRESETS_JSON).expect("embedded presets are JSON");
        let mutate = |patch: fn(&mut Value)| {
            let mut value = base.clone();
            patch(&mut value);
            presets_from_json(&value.to_string())
        };
        assert!(mutate(|value| value["version"] = json!(0)).is_err());
        assert!(mutate(|value| value["default_region"] = json!("MARS")).is_err());
        assert!(mutate(|value| value["country_regions"]["NO"] = json!("MARS")).is_err());
        assert!(
            mutate(|value| { value["roles"]["client"]["profile"] = json!("HYPER_FAST") }).is_err()
        );
        assert!(
            mutate(|value| { value["roles"]["client"]["position_sharing"] = json!("everywhere") })
                .is_err()
        );
        assert!(
            mutate(|value| value["roles"]["client"]["announce_interval_s"] = json!(10)).is_err()
        );
        assert!(
            mutate(|value| {
                value["roles"].as_object_mut().unwrap().remove("gateway");
            })
            .is_err()
        );
    }

    #[test]
    fn recommend_uses_country_then_current_region() {
        let current = config();
        let value = recommend(Some("US"), Some("repeater"), &current);
        assert_eq!(value["region"], "US915");
        assert_eq!(value["profile"], "LONG_FAST");
        assert_eq!(value["store_forward"], true);
        assert_eq!(value["duty_cycle_pct"], Value::Null);
        let fallback = recommend(None, None, &current);
        assert_eq!(fallback["region"], current.radio.region);
        assert_eq!(fallback["role"], "client");
        let unknown = recommend(Some("ZZ"), Some("client"), &current);
        assert_eq!(unknown["region"], presets().default_region);
        assert!(unknown["warning"].is_string());
    }
}
