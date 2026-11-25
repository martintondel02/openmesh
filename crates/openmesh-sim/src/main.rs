//! `openmesh-sim` — run the OpenMesh mesh scenarios or reproducible randomized experiments.
//!
//! ```text
//! openmesh-sim                          # all scenarios, human report, non-zero on failure
//! openmesh-sim --scenario relay_chain_5 # one scenario
//! openmesh-sim --nodes 25 --seed 42     # randomized mesh experiment
//! openmesh-sim --nodes 25 --seed 42 --json
//! openmesh-sim --list
//! ```

use std::process::ExitCode;

use clap::Parser;
use openmesh_sim::Report;
use openmesh_sim::scenarios;

#[derive(Debug, Parser)]
#[command(
    name = "openmesh-sim",
    version,
    about = "OpenMesh discrete-event mesh simulator: scenarios with assertions and reproducible experiments"
)]
struct Args {
    /// Run one built-in scenario instead of all of them.
    #[arg(long, value_name = "NAME")]
    scenario: Option<String>,

    /// List the built-in scenarios and exit.
    #[arg(long)]
    list: bool,

    /// Randomized experiment mode: number of virtual nodes.
    #[arg(long, value_name = "N")]
    nodes: Option<usize>,

    /// RNG seed for reproducible experiments (also used by the built-in scenarios).
    #[arg(long, value_name = "SEED")]
    seed: Option<u64>,

    /// Print machine-readable JSON only.
    #[arg(long)]
    json: bool,

    /// Include per-node rows in the human report (all scenarios print them for small meshes).
    #[arg(long)]
    verbose: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();

    if args.list {
        for name in scenarios::SCENARIOS {
            println!("{name}");
        }
        return ExitCode::SUCCESS;
    }

    let reports: Vec<Report> = if let Some(nodes) = args.nodes {
        let seed = args.seed.unwrap_or(42);
        vec![scenarios::random_mesh_experiment(nodes, seed)]
    } else if let Some(name) = args.scenario.as_deref() {
        match scenarios::run_named(name) {
            Some(report) => vec![report],
            None => {
                eprintln!("openmesh-sim: unknown scenario '{name}'");
                eprintln!("available scenarios:");
                for name in scenarios::SCENARIOS {
                    eprintln!("  {name}");
                }
                return ExitCode::from(2);
            }
        }
    } else {
        scenarios::run_all()
    };

    if report_failed(&reports) {
        if args.json {
            let payload: Vec<serde_json::Value> = reports.iter().map(Report::to_json).collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&payload).unwrap_or_default()
            );
        } else {
            for report in &reports {
                print!("{}", report.human());
                println!();
            }
            print_summary(&reports);
        }
        return ExitCode::from(1);
    }

    if args.json {
        let payload: Vec<serde_json::Value> = reports.iter().map(Report::to_json).collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).unwrap_or_default()
        );
    } else {
        for report in &reports {
            print!("{}", report.human());
            let _ = args.verbose;
            println!();
        }
        print_summary(&reports);
    }
    ExitCode::SUCCESS
}

fn report_failed(reports: &[Report]) -> bool {
    reports.iter().any(|r| !r.passed())
}

fn print_summary(reports: &[Report]) {
    let passed = reports.iter().filter(|r| r.passed()).count();
    let total = reports.len();
    let messages: usize = reports.iter().map(|r| r.messages_delivered).sum();
    let airtime: u64 = reports.iter().map(|r| r.airtime_total_ms).sum();
    println!(
        "summary: {passed}/{total} scenarios passed, {messages} messages delivered, {} simulated airtime",
        openmesh_core::util::format_duration_ms(airtime)
    );
    if passed == total {
        println!("OK");
    } else {
        println!("FAILED");
    }
}
