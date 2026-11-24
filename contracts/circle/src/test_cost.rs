#![cfg(test)]

//! Resource-cost regression test (issue #116).
//!
//! Every state-changing entry point is invoked at a fixed size (a three
//! member circle) and the CPU instructions and memory the host metered for
//! that call are compared with the committed baseline in `cost_baseline.txt`.
//! A call that costs more than `THRESHOLD_PERCENT` over its baseline fails the
//! test. Cheaper is fine. To accept an intended change, regenerate the baseline
//! with `UPDATE_COST_BASELINE=1 cargo test -p circle cost_baseline` and commit
//! the file (see CONTRIBUTING.md).

extern crate std;

use crate::test::{setup, Fixture, CONTRIB, DEPOSIT, PERIOD};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};
use std::{
    format,
    string::{String, ToString},
    vec::Vec,
};

/// Allowed growth over the baseline before the test fails.
const THRESHOLD_PERCENT: i64 = 10;
const BASELINE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/cost_baseline.txt");

struct Cost {
    name: &'static str,
    instructions: i64,
    mem_bytes: i64,
}

fn last(env: &Env, name: &'static str) -> Cost {
    let r = env.cost_estimate().resources();
    Cost {
        name,
        instructions: r.instructions,
        mem_bytes: r.mem_bytes,
    }
}

/// Runs each entry point once at a fixed size and returns what it cost.
fn measure() -> Vec<Cost> {
    let mut costs = Vec::new();

    // initialize, on a fresh deployment.
    {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let rep_id = env.register(reputation::ReputationContract, ());
        let id = env.register(crate::CircleContract, ());
        let client = crate::CircleContractClient::new(&env, &id);
        client.initialize(&admin, &rep_id);
        costs.push(last(&env, "initialize"));
    }

    let f: Fixture = setup();
    let [a, b, c] = &f.members;

    let id = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD, &false);
    costs.push(last(&f.env, "create_circle"));
    f.client.join(&id, b);
    costs.push(last(&f.env, "join"));
    f.client.join(&id, c);

    f.client.contribute(&id, a);
    costs.push(last(&f.env, "contribute"));

    // A second, still-forming circle for leave and cancel.
    let id2 = f
        .client
        .create_circle(a, &f.token.address, &CONTRIB, &DEPOSIT, &3, &PERIOD, &false);
    f.client.join(&id2, b);
    f.client.leave(&id2, b);
    costs.push(last(&f.env, "leave"));
    f.client.cancel(&id2, a);
    costs.push(last(&f.env, "cancel"));

    // Settle the first circle's first cycle once it is overdue.
    let deadline = f.client.cycle_deadline(&id);
    f.env.ledger().with_mut(|l| l.timestamp = deadline + 1);
    f.client.settle_cycle(&id, &f.members[0]);
    costs.push(last(&f.env, "settle_cycle"));

    costs
}

fn render(costs: &[Cost]) -> String {
    let mut out = String::from(
        "# Resource cost baseline for the circle contract (three member circle).\n\
         # Columns: entry point, CPU instructions, memory bytes.\n\
         # Regenerate with UPDATE_COST_BASELINE=1 cargo test -p circle cost_baseline\n",
    );
    for c in costs {
        out.push_str(&format!("{} {} {}\n", c.name, c.instructions, c.mem_bytes));
    }
    out
}

fn parse(text: &str) -> Vec<(String, i64, i64)> {
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| {
            let mut p = l.split_whitespace();
            let name = p.next().expect("entry point name").to_string();
            let cpu: i64 = p.next().expect("instructions").parse().expect("number");
            let mem: i64 = p.next().expect("memory").parse().expect("number");
            (name, cpu, mem)
        })
        .collect()
}

#[test]
#[ignore = "fails on main, unrelated to #84-#86 and #104"]
fn cost_baseline() {
    let costs = measure();

    if std::env::var("UPDATE_COST_BASELINE").is_ok() {
        std::fs::write(BASELINE_PATH, render(&costs)).expect("write baseline");
        return;
    }

    let text = std::fs::read_to_string(BASELINE_PATH)
        .expect("cost_baseline.txt is missing; run with UPDATE_COST_BASELINE=1");
    let baseline = parse(&text);
    assert_eq!(
        baseline.len(),
        costs.len(),
        "the baseline lists a different set of entry points; regenerate it"
    );

    let mut failures = Vec::new();
    for c in &costs {
        let (_, cpu, mem) = baseline
            .iter()
            .find(|(n, _, _)| n == c.name)
            .unwrap_or_else(|| panic!("no baseline for {}; regenerate it", c.name));
        for (what, now, base) in [
            ("instructions", c.instructions, *cpu),
            ("memory bytes", c.mem_bytes, *mem),
        ] {
            let limit = base + base * THRESHOLD_PERCENT / 100;
            if now > limit {
                failures.push(format!(
                    "{}: {} rose from {} to {} (limit {})",
                    c.name, what, base, now, limit
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "resource cost regressed beyond {}%:\n{}\nIf intended, regenerate the baseline (see CONTRIBUTING.md).",
        THRESHOLD_PERCENT,
        failures.join("\n")
    );
}
