//! First-run checklist, region/role recommendations and the secure beginner preset.
//!
//! Normative sources: `docs/design/ux-help-system.md` §3/§5 and `docs/architecture/AI-NATIVE.md`
//! (MCP `openmesh_setup_state` / `openmesh_setup_quick`). Presets are server-owned so the web
//! wizard, CLI and agents behave identically.

use serde_json::{Map, Value, json};

use openmesh_core::util;

use crate::json::{ChannelCreateData, ConfigJson, b64_decode, parse_node_id};
use crate::runtime::{self, CommandError, CommandKind, Shared};
use crate::transport::backbone::BackboneManager;

/// Roles accepted by the beginner wizard.
pub const ROLES: &[&str] = &["client", "tracker", "repeater", "gateway"];

/// Server-owned preset for one role (UX spec §5 table).
#[derive(Clone, Copy, Debug)]
pub struct RolePreset {
    pub role: &'static str,
    pub profile: &'static str,
    pub store_forward: bool,
    pub position_sharing: bool,
    pub announce_interval_s: u64,
}

pub fn role_preset(role: &str) -> RolePreset {
    match role {
        "tracker" => RolePreset {
            role: "tracker",
            profile: "MEDIUM_FAST",
            store_forward: false,
            position_sharing: true,
            announce_interval_s: 3600,
        },
        "repeater" => RolePreset {
            role: "repeater",
            profile: "LONG_FAST",
            store_forward: true,
            position_sharing: false,
            announce_interval_s: 3600,
        },
        "gateway" => RolePreset {
            role: "gateway",
            profile: "LONG_FAST",
            store_forward: true,
            position_sharing: false,
            announce_interval_s: 3600,
        },
        _ => RolePreset {
            role: "client",
            profile: "MEDIUM_FAST",
            store_forward: false,
            position_sharing: false,
            announce_interval_s: 1800,
        },
    }
}

/// Map a country code to a regulatory region (UX spec §5), with a warning for unknown codes.
pub fn country_region(country: &str) -> (&'static str, Option<String>) {
    let code = country.trim().to_ascii_uppercase();
    let region = match code.as_str() {
        // Europe (EU/EEA/UK + EFTA)
        "EU" | "UK" | "GB" | "NO" | "CH" | "IS" | "LI" | "AT" | "BE" | "BG" | "HR" | "CY"
        | "CZ" | "DK" | "EE" | "FI" | "FR" | "DE" | "GR" | "HU" | "IE" | "IT" | "LV" | "LT"
        | "LU" | "MT" | "NL" | "PL" | "PT" | "RO" | "SK" | "SI" | "ES" | "SE" => "EU868",
        "US" | "CA" | "MX" => "US915",
        "AU" | "NZ" => "AU915",
        "IN" => "IN865",
        "KR" => "KR920",
        "SG" | "MY" | "TH" | "ID" | "PH" | "VN" => "AS923",
        _ => {
            return (
                "EU868",
                Some(format!(
                    "Unknown country '{code}'; defaulting to EU868. Pick your region in Settings if that is wrong."
                )),
            );
        }
    };
    (region, None)
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
    match role.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("repeater") => "repeater",
        Some("gateway") => "gateway",
        Some("tracker") => "tracker",
        Some("client") => "client",
        _ => infer_role(&current.node.class),
    }
}

fn region_info(region: &str) -> Option<&'static util::Region> {
    util::REGIONS.iter().find(|info| info.name == region)
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
    json!({
        "country": country,
        "role": role,
        "region": region,
        "profile": preset.profile,
        "store_forward": preset.store_forward,
        "position_sharing": preset.position_sharing,
        "announce_interval_s": preset.announce_interval_s,
        "duty_cycle_pct": info.and_then(|info| info.duty_cycle_pct),
        "frequency_mhz": info.map(|info| info.freq_khz as f64 / 1000.0),
        "notes": notes,
        "warning": warning,
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
        },
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
    if !ROLES.contains(&role.as_str()) {
        return Err(bad_request(
            "role must be one of client, tracker, repeater, gateway",
        ));
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
        // Position: tracker shares (channel-only in the UI); client only when privacy=shared;
        // repeaters/gateways keep it off.
        let position_sharing = match role.as_str() {
            "tracker" => true,
            "client" => privacy == "shared",
            _ => false,
        };
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
    fn country_mapping_matches_ux_spec() {
        assert_eq!(country_region("NO").0, "EU868");
        assert_eq!(country_region("us").0, "US915");
        assert_eq!(country_region("AU").0, "AU915");
        assert_eq!(country_region("IN").0, "IN865");
        assert_eq!(country_region("KR").0, "KR920");
        assert_eq!(country_region("SG").0, "AS923");
        let (region, warning) = country_region("ZZ");
        assert_eq!(region, "EU868");
        assert!(warning.is_some());
    }

    #[test]
    fn role_presets_match_spec_table() {
        let client = role_preset("client");
        assert_eq!(client.profile, "MEDIUM_FAST");
        assert!(!client.store_forward);
        let repeater = role_preset("repeater");
        assert_eq!(repeater.profile, "LONG_FAST");
        assert!(repeater.store_forward);
        assert!(role_preset("tracker").position_sharing);
        assert!(!role_preset("gateway").position_sharing);
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
    }
}
