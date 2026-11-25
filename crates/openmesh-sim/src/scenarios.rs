//! Built-in simulator scenarios with assertions and human-readable reports.
//!
//! Each scenario is a small, deterministic experiment: it builds a [`MeshSim`], drives it with
//! real `openmesh_core::Engine` nodes and asserts the protocol behaviour documented in
//! `docs/architecture/PROTOCOL.md`. `cargo run -p openmesh-sim` runs them all and exits non-zero
//! when any assertion fails.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use openmesh_core::util;

use crate::{Assertion, MeshSim, Position, Report, SimConfig};

/// Names of all built-in scenarios, newest first in the report.
pub const SCENARIOS: &[&str] = &[
    "two_node_chat",
    "relay_chain_5",
    "lossy_retransmit_20pct",
    "offline_store_forward",
    "image_5kib_profile_costs",
    "announce_convergence",
];

/// Run every built-in scenario.
pub fn run_all() -> Vec<Report> {
    SCENARIOS
        .iter()
        .filter_map(|name| run_named(name))
        .collect()
}

/// Run one scenario by name.
pub fn run_named(name: &str) -> Option<Report> {
    Some(match name {
        "two_node_chat" => two_node_chat(),
        "relay_chain_5" => relay_chain_5(),
        "lossy_retransmit_20pct" => lossy_retransmit_20pct(),
        "offline_store_forward" => offline_store_forward(),
        "image_5kib_profile_costs" => image_5kib_profile_costs(),
        "announce_convergence" => announce_convergence(),
        _ => return None,
    })
}

/// Two handhelds in direct range: send, deliver, ack. The smallest end-to-end contract.
pub fn two_node_chat() -> Report {
    let mut sim = MeshSim::new(
        2,
        SimConfig {
            seed: 42,
            latency_ms: 15,
            loss_pct: 0,
            ..Default::default()
        },
    );
    sim.set_full_topology();
    assert!(sim.converge_announces(10_000), "announces must converge");
    sim.run_until(sim.now_ms + 100);

    let msg_id = sim.send_text(0, 1, "hello mesh").expect("send_text");
    let delivery = sim.run_until_cond(3_000, |sim| {
        sim.deliveries().iter().any(|d| d.msg_id == msg_id)
    });
    // Give the delivery ack time to come back before asserting on it.
    let acked = sim.run_until_cond(5_000, |sim| {
        sim.acks()
            .iter()
            .any(|a| a.msg_id == msg_id && a.kind == "delivered")
    });
    let message = sim.delivered_text(msg_id).cloned();

    let mut assertions = Vec::new();
    assertions.push(Assertion::ok(
        "message delivered",
        delivery && message.is_some(),
        match &message {
            Some(m) => format!("node 0 -> node 1 in {}ms", m.delivered_at_ms),
            None => "message never arrived".to_string(),
        },
    ));
    assertions.push(Assertion::ok(
        "text intact",
        message.as_ref().and_then(|m| m.text.as_deref()) == Some("hello mesh"),
        format!("text={:?}", message.as_ref().and_then(|m| m.text.clone())),
    ));
    assertions.push(Assertion::ok(
        "delivery ack returned",
        acked,
        format!("acks={} ", sim.acks().len()),
    ));
    assertions.push(Assertion::ok(
        "one hop path",
        message.as_ref().and_then(|m| m.hops) == Some(1),
        format!("hops={:?}", message.as_ref().and_then(|m| m.hops)),
    ));
    sim.report("two_node_chat", assertions)
}

/// A five-node chain where only the middle hops can relay; the message must take four hops.
pub fn relay_chain_5() -> Report {
    let mut sim = MeshSim::new(
        5,
        SimConfig {
            seed: 7,
            latency_ms: 20,
            loss_pct: 0,
            ..Default::default()
        },
    );
    sim.set_line_topology();
    assert!(
        sim.converge_announces(20_000),
        "announces must reach the far end of the chain"
    );
    sim.run_until(sim.now_ms + 100);

    let msg_id = sim
        .send_text(0, 4, "over the ridge and far away")
        .expect("send_text");
    let delivered = sim.run_until_cond(10_000, |sim| {
        sim.deliveries().iter().any(|d| d.msg_id == msg_id)
    });
    let message = sim.delivered_text(msg_id).cloned();
    let relays_tx: u64 = sim.nodes[1..4].iter().map(|n| n.tx_frames).sum();

    let mut assertions = Vec::new();
    assertions.push(Assertion::ok(
        "message crosses the chain",
        delivered,
        match &message {
            Some(m) => format!(
                "delivered in {}ms over {:?} hops",
                m.delivered_at_ms, m.hops
            ),
            None => "not delivered".to_string(),
        },
    ));
    assertions.push(Assertion::ok(
        "four hop path",
        message.as_ref().and_then(|m| m.hops) == Some(4),
        format!("hops={:?}", message.as_ref().and_then(|m| m.hops)),
    ));
    assertions.push(Assertion::ok(
        "intermediate nodes relayed",
        relays_tx > 0,
        format!("relay-node transmissions={relays_tx}"),
    ));
    sim.report("relay_chain_5", assertions)
}

/// 20% packet loss: the message must still arrive thanks to MAC retries / protocol retransmits.
pub fn lossy_retransmit_20pct() -> Report {
    let mut sim = MeshSim::new(
        2,
        SimConfig {
            seed: 11,
            latency_ms: 25,
            loss_pct: 20,
            ..Default::default()
        },
    );
    sim.set_full_topology();
    assert!(
        sim.converge_announces(60_000),
        "announces must get through despite 20% loss"
    );

    // A batch of messages gives the retry logic a chance to do its job on a lossy link.
    let mut sent = Vec::new();
    for i in 0..5 {
        let msg_id = sim
            .send_text(0, 1, &format!("lossy message #{i}"))
            .expect("send_text");
        sent.push(msg_id);
        sim.run_until(sim.now_ms + 1_500);
    }
    sim.run_until_cond(120_000, |sim| {
        sent.iter().all(|id| {
            sim.acks()
                .iter()
                .any(|a| a.msg_id == *id && a.kind == "delivered")
        })
    });
    let delivered_ids: Vec<u32> = sim
        .deliveries()
        .iter()
        .filter(|d| sent.contains(&d.msg_id))
        .map(|d| d.msg_id)
        .collect();
    let all_delivered = delivered_ids.len() == sent.len();
    let acked = sent
        .iter()
        .filter(|id| {
            sim.acks()
                .iter()
                .any(|a| a.msg_id == **id && a.kind == "delivered")
        })
        .count();
    let retransmits = sim.total_retransmits();
    let dropped = sim.dropped_loss;

    let mut assertions = Vec::new();
    assertions.push(Assertion::ok(
        "all messages delivered despite loss",
        all_delivered,
        format!(
            "{:.0}% loss, {} frames dropped, {}/{} delivered",
            sim.config.loss_pct,
            dropped,
            delivered_ids.len(),
            sent.len()
        ),
    ));
    assertions.push(Assertion::ok(
        "delivery acks returned",
        acked == sent.len(),
        format!("{acked}/{} acks", sent.len()),
    ));
    assertions.push(Assertion::ok(
        "retransmissions happened",
        retransmits > 0,
        format!("retransmits={retransmits}"),
    ));
    sim.report("lossy_retransmit_20pct", assertions)
}

/// Receiver is offline for five virtual minutes; a repeater must store the ciphertext and replay
/// it when the receiver wakes up and announces.
pub fn offline_store_forward() -> Report {
    // The relay only stores envelopes for a peer it has not heard from for
    // STORE_PEER_REACHABLE_MS (5 min), so the recipient must be absent for a while before the
    // message is sent — exactly like a handheld left at the cabin.
    let absent_before_send_ms: u64 = 6 * 60_000;
    let offline_after_send_ms: u64 = 2 * 60_000;
    let mut sim = MeshSim::new(
        3,
        SimConfig {
            seed: 23,
            latency_ms: 20,
            loss_pct: 0,
            ..Default::default()
        },
    );
    sim.set_line_topology();
    sim.set_repeater(1);
    assert!(
        sim.converge_announces(20_000),
        "announces must converge before the peer goes offline"
    );

    let known_before = sim.nodes[0].engine.node_info(sim.node_id(2)).is_some();
    sim.set_online(2, false);
    sim.run_until(sim.now_ms + absent_before_send_ms);
    let offline_start = sim.now_ms;

    let msg_id = sim
        .send_text(0, 2, "left the cabin, will check in later")
        .expect("send_text");
    // Keep the mesh running (repeater ticks, sender retries) while the peer is away.
    sim.run_until(offline_start + offline_after_send_ms);
    let relay_stored = sim.nodes[1].engine.stats().stored_envelopes;
    sim.set_online(2, true);
    let wake_time = sim.now_ms;
    let delivered = sim.run_until_cond(wake_time + 900_000, |sim| {
        sim.deliveries().iter().any(|d| d.msg_id == msg_id)
    });
    let message = sim.delivered_text(msg_id).cloned();

    let mut assertions = Vec::new();
    assertions.push(Assertion::ok(
        "receiver was known before going offline",
        known_before,
        format!("known_before={known_before}"),
    ));
    assertions.push(Assertion::ok(
        "message delivered after wake-up",
        delivered,
        match &message {
            Some(m) => format!(
                "delivered at t={}ms, {} after the wake-up",
                m.delivered_at_ms,
                util::format_duration_ms(m.delivered_at_ms.saturating_sub(wake_time))
            ),
            None => format!(
                "still missing after {} offline",
                util::format_duration_ms(absent_before_send_ms + offline_after_send_ms)
            ),
        },
    ));
    assertions.push(Assertion::ok(
        "repeater held the envelope while the peer was offline",
        relay_stored > 0,
        format!("repeater stored_envelopes={relay_stored}"),
    ));
    assertions.push(Assertion::ok(
        "delivery completed through store-and-forward",
        sim.store_forward_deliveries() >= 1,
        format!(
            "store-forward deliveries={}",
            sim.store_forward_deliveries()
        ),
    ));
    sim.report("offline_store_forward", assertions)
}

/// 5 KiB attachment over the MEDIUM_FAST profile: verify reassembly and report honest airtime.
pub fn image_5kib_profile_costs() -> Report {
    let mut sim = MeshSim::new(
        2,
        SimConfig {
            seed: 29,
            latency_ms: 20,
            loss_pct: 0,
            ..Default::default()
        },
    );
    sim.set_full_topology();
    assert!(sim.converge_announces(10_000), "announces must converge");

    let data = pseudo_random_bytes(5120, 0x1234_5678);
    let (msg_id, res_id) = sim
        .send_resource(0, 1, "image/jpeg", &data)
        .expect("send_resource");
    let delivered = sim.run_until_cond(120_000, |sim| {
        sim.resource_deliveries().iter().any(|r| r.res_id == res_id)
    });
    let delivery = sim
        .resource_deliveries()
        .iter()
        .find(|r| r.res_id == res_id)
        .cloned();
    let manifest = sim
        .deliveries()
        .iter()
        .find(|d| d.msg_id == msg_id)
        .cloned();
    let frames_for_transfer = sim.nodes[0].tx_frames;
    let airtime = sim.total_airtime_ms();
    let one_frame = util::lora_airtime_ms(&sim.config.profile, 240);

    let mut assertions = Vec::new();
    assertions.push(Assertion::ok(
        "attachment manifest delivered",
        manifest.is_some(),
        format!("manifest msg {} present", msg_id),
    ));
    assertions.push(Assertion::ok(
        "attachment reassembled",
        delivered && delivery.as_ref().map(|d| d.bytes) == Some(data.len()),
        match &delivery {
            Some(d) => format!("{} bytes in {}ms", d.bytes, d.at_ms),
            None => "transfer did not complete".to_string(),
        },
    ));
    assertions.push(Assertion::ok(
        "payload integrity verified by the engine",
        delivery.as_ref().map(|d| d.sha_ok).unwrap_or(false),
        format!("verified={:?}", delivery.as_ref().map(|d| d.sha_ok)),
    ));
    assertions.push(Assertion::ok(
        "chunked into >=29 frames",
        frames_for_transfer >= 29,
        format!(
            "sender transmitted {frames_for_transfer} frames (one frame ≈ {}ms airtime)",
            one_frame
        ),
    ));
    assertions.push(Assertion::ok(
        "airtime is LoRa-plausible",
        (10_000..=120_000).contains(&airtime),
        format!(
            "{} total airtime for 5 KiB at {}",
            util::format_duration_ms(airtime),
            sim.config.profile.name
        ),
    ));
    sim.report("image_5kib_profile_costs", assertions)
}

/// Eight nodes on a random geometric graph: every node must learn every other node via announces.
pub fn announce_convergence() -> Report {
    let nodes = 8;
    let mut sim = MeshSim::new(
        nodes,
        SimConfig {
            seed: 31,
            latency_ms: 20,
            loss_pct: 0,
            ..Default::default()
        },
    );
    sim.set_topology(&geometric_topology(nodes, 31, 520.0));
    let start = sim.now_ms;
    sim.announce_all();
    let converged = sim.run_until_cond(30_000, |sim| sim.fully_converged_nodes() == nodes);
    let convergence_ms = sim
        .discovery
        .iter()
        .flat_map(|seen| seen.values().copied())
        .max()
        .unwrap_or(0)
        .saturating_sub(start);
    let per_node: Vec<(String, usize)> = sim
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.name.clone(), sim.discovery[i].len()))
        .collect();

    let mut assertions = Vec::new();
    assertions.push(Assertion::ok(
        "all nodes discovered each other",
        converged,
        format!(
            "{}/{} nodes fully converged in {}",
            sim.fully_converged_nodes(),
            nodes,
            util::format_duration_ms(convergence_ms)
        ),
    ));
    assertions.push(Assertion::ok(
        "convergence under 30s",
        converged && convergence_ms < 30_000,
        format!("took {}ms", convergence_ms),
    ));
    assertions.push(Assertion::ok(
        "every node has the full node table",
        per_node.iter().all(|(_, count)| *count >= nodes - 1),
        format!("{per_node:?}"),
    ));
    sim.report("announce_convergence", assertions)
}

/// Reproducible experiment mode: `--nodes 25 --seed 42` builds a random mesh, converges announces,
/// sends messages between random reachable pairs and reports delivery/airtime statistics.
pub fn random_mesh_experiment(nodes: usize, seed: u64) -> Report {
    let nodes = nodes.max(2);
    let mut sim = MeshSim::new(
        nodes,
        SimConfig {
            seed,
            latency_ms: 20,
            loss_pct: 0,
            ..Default::default()
        },
    );
    let range = 1000.0 * 1.9 / (nodes as f64).sqrt();
    sim.set_topology(&geometric_topology(nodes, seed, range));

    sim.announce_all();
    let converged = sim.run_until_cond(30_000, |sim| sim.fully_converged_nodes() == nodes);

    // Send a batch of messages between reachable random pairs.
    let mut rng = StdRng::seed_from_u64(seed ^ 0xA5A5_5A5A);
    let mut expected = 0usize;
    for _ in 0..25 {
        let mut from = rng.gen_range(0..nodes);
        let mut to = rng.gen_range(0..nodes);
        let mut attempts = 0;
        while (from == to || sim.hops_between(from, to).is_none()) && attempts < 100 {
            from = rng.gen_range(0..nodes);
            to = rng.gen_range(0..nodes);
            attempts += 1;
        }
        if from == to || sim.hops_between(from, to).is_none() {
            continue;
        }
        if sim
            .send_text(from, to, &format!("experiment message {expected}"))
            .is_ok()
        {
            expected += 1;
        }
    }
    sim.run_until(sim.now_ms + 60_000);
    let delivered = sim.deliveries().len();
    let pct = if expected > 0 {
        delivered as f64 / expected as f64 * 100.0
    } else {
        0.0
    };
    let assertions = vec![
        Assertion::ok(
            "announce convergence",
            converged,
            format!("{}/{} fully converged", sim.fully_converged_nodes(), nodes),
        ),
        Assertion::ok(
            "message delivery",
            pct >= 95.0,
            format!("{delivered}/{expected} messages delivered ({pct:.1}%)"),
        ),
    ];
    sim.report(
        &format!("random_mesh_{nodes}_nodes_seed_{seed}"),
        assertions,
    )
}

/// Deterministic pseudo-random topology (random geometric graph), repaired to be connected.
pub fn geometric_topology(nodes: usize, seed: u64, range: f64) -> Vec<(usize, usize)> {
    let mut rng = StdRng::seed_from_u64(seed);
    let positions: Vec<(f64, f64)> = (0..nodes)
        .map(|_| (rng.gen_range(0.0..1000.0), rng.gen_range(0.0..1000.0)))
        .collect();
    let mut edges = Vec::new();
    let mut adjacency = vec![Vec::new(); nodes];
    for i in 0..nodes {
        for j in (i + 1)..nodes {
            let dx = positions[i].0 - positions[j].0;
            let dy = positions[i].1 - positions[j].1;
            if (dx * dx + dy * dy).sqrt() <= range {
                edges.push((i, j));
                adjacency[i].push(j);
                adjacency[j].push(i);
            }
        }
    }
    // Repair connectivity so experiments are not dominated by isolated nodes.
    let mut reachable = vec![false; nodes];
    let mut stack = vec![0usize];
    reachable[0] = true;
    while let Some(node) = stack.pop() {
        for &next in &adjacency[node] {
            if !reachable[next] {
                reachable[next] = true;
                stack.push(next);
            }
        }
    }
    for i in 0..nodes {
        if reachable[i] {
            continue;
        }
        // Attach the unreachable node to the closest reachable one.
        let mut best = 0usize;
        let mut best_dist = f64::MAX;
        for (j, position) in positions.iter().enumerate().take(nodes) {
            if j != i && reachable[j] {
                let dx = positions[i].0 - position.0;
                let dy = positions[i].1 - position.1;
                let dist = dx * dx + dy * dy;
                if dist < best_dist {
                    best_dist = dist;
                    best = j;
                }
            }
        }
        edges.push((best, i));
        adjacency[best].push(i);
        adjacency[i].push(best);
        reachable[i] = true;
        stack.push(i);
        while let Some(node) = stack.pop() {
            for &next in &adjacency[node] {
                if !reachable[next] {
                    reachable[next] = true;
                    stack.push(next);
                }
            }
        }
    }
    edges
}

fn pseudo_random_bytes(len: usize, seed: u32) -> Vec<u8> {
    let mut x = seed;
    (0..len)
        .map(|_| {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (x >> 24) as u8
        })
        .collect()
}

/// Helper for scenarios that want a position with sensible defaults.
pub fn sample_position(lat_e7: i32, lon_e7: i32) -> Position {
    Position {
        lat_e7,
        lon_e7,
        alt_m: Some(120),
        accuracy_m: Some(10),
        speed_cm_s: None,
        heading_deg: None,
        timestamp_ms: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_scenario_passes() {
        for name in SCENARIOS {
            let report = run_named(name).unwrap_or_else(|| panic!("missing scenario {name}"));
            if let Some(failure) = report.failures().first() {
                panic!(
                    "scenario {name}: assertion '{}' failed: {}",
                    failure.name, failure.detail
                );
            }
        }
    }

    #[test]
    fn scenario_names_are_unique_and_known() {
        let mut names = SCENARIOS.to_vec();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), SCENARIOS.len());
        assert!(run_named("nope").is_none());
    }

    #[test]
    fn geometric_topology_is_connected() {
        for nodes in [2usize, 5, 12, 25] {
            let edges = geometric_topology(nodes, 99, 200.0);
            let mut sim = MeshSim::new(nodes, SimConfig::default());
            sim.set_topology(&edges);
            for i in 1..nodes {
                assert!(sim.hops_between(0, i).is_some(), "node {i} unreachable");
            }
        }
    }

    #[test]
    fn random_experiment_is_reproducible() {
        let a = random_mesh_experiment(8, 42);
        let b = random_mesh_experiment(8, 42);
        assert_eq!(a.messages_delivered, b.messages_delivered);
        assert_eq!(a.frames_tx, b.frames_tx);
        assert!(a.passed());
    }
}
