//! Settlement logic for the circle contract.

use core::cmp::min;

use soroban_sdk::token;
use soroban_sdk::{panic_with_error, vec, Address, Env, Vec};

use crate::errors::Error;
use crate::types::{CircleStatus, GlobalConfig};
use crate::ReputationClient;

/// Settle a due cycle. `keeper` is the address that triggered settlement and
/// receives a 1-unit reward from slashed funds when any slash occurred.
pub fn settle_cycle(env: Env, circle_id: u32, keeper: Address) {
    keeper.require_auth();
    let mut circle = crate::storage::read_circle(&env, circle_id);
    if circle.status != CircleStatus::Active {
        panic_with_error!(&env, Error::BadStatus);
    }
    let members = crate::storage::read_members(&env, circle_id);
    let cycle = circle.current_cycle;
    let now = env.ledger().timestamp();
    let deadline = circle.started_at + (cycle as u64 + 1) * circle.period_secs;

    // Due when every active member has paid, or the deadline has passed.
    let mut all_paid = true;
    for m in members.iter() {
        let st = crate::storage::read_member(&env, circle_id, &m);
        if st.defaulted {
            continue;
        }
        if !crate::storage::is_paid(&env, circle_id, cycle, &m) {
            all_paid = false;
            break;
        }
    }
    crate::validation::validate_cycle_due(all_paid, now, deadline, &env);

    let config: GlobalConfig = crate::storage::read_config(&env);
    let tok = token::Client::new(&env, &circle.token);
    let rep = ReputationClient::new(&env, &config.reputation);
    let this = env.current_contract_address();

    // 1) Collect the pot: contributions already escrowed by payers, plus
    //    slashes from active members who missed.
    let mut pot: i128 = 0;
    let mut cycle_slash: i128 = 0;
    let mut payers: Vec<Address> = vec![&env];
    let mut slashed_who: Vec<Address> = vec![&env];
    let mut slashed_amt: Vec<i128> = vec![&env];
    for m in members.iter() {
        let mut st = crate::storage::read_member(&env, circle_id, &m);
        if st.defaulted {
            continue;
        }
        if crate::storage::is_paid(&env, circle_id, cycle, &m) {
            pot += circle.contribution;
            payers.push_back(m.clone());
        } else {
            let slash = min(st.deposit_remaining, circle.contribution);
            if slash > 0 {
                st.deposit_remaining -= slash;
                pot += slash;
                cycle_slash += slash;
                circle.slashed_total += slash;
                slashed_who.push_back(m.clone());
                slashed_amt.push_back(slash);
                crate::events::slash(&env, circle_id, &m, slash);
            }
            if slash < circle.contribution {
                st.defaulted = true;
                crate::events::defaulted(&env, circle_id, &m);
                rep.report_default(&this, &m, &(circle_id as u64));
            }
            crate::storage::write_member(&env, circle_id, &m, &st);
        }
    }

    // 1b) Pay keeper a 1-unit incentive from slashed funds when any slash occurred.
    // Skip when nobody contributed: the None/n==0 branch returns full slash amounts
    // back to the slashed members, so removing 1 unit here would leave the contract 1 short.
    if cycle_slash > 0 && pot > 1 && !payers.is_empty() {
        tok.transfer(&this, &keeper, &1i128);
        pot -= 1;
        crate::events::keeper_reward(&env, circle_id, &keeper, 1i128);
    }

    // 2) Pay the first member in payout order who is active and unpaid.
    let payout_order = crate::storage::read_payout_order(&env, circle_id);
    let mut recipient: Option<Address> = None;
    for m in payout_order.iter() {
        let st = crate::storage::read_member(&env, circle_id, &m);
        if !st.received && !st.defaulted {
            recipient = Some(m);
            break;
        }
    }
    match recipient {
        Some(r) => {
            if pot > 0 {
                tok.transfer(&this, &r, &pot);
            }
            let mut st = crate::storage::read_member(&env, circle_id, &r);
            st.received = true;
            crate::storage::write_member(&env, circle_id, &r, &st);
            crate::events::payout(&env, circle_id, &r, pot, cycle);
        }
        None => {
            // Edge: this settlement's defaults eliminated every remaining
            // unpaid member. Return contributions to payers and split the
            // slashed amount equally among them (remainder to the first),
            // so no funds strand in the contract.
            let n = payers.len();
            if n > 0 {
                let slash_total = pot - circle.contribution * (n as i128);
                let share = slash_total / (n as i128);
                let remainder = slash_total - share * (n as i128);
                for (i, p) in (0_u32..).zip(payers.iter()) {
                    let mut amount = circle.contribution + share;
                    if i == 0 {
                        amount += remainder;
                    }
                    if amount > 0 {
                        tok.transfer(&this, &p, &amount);
                    }
                }
            } else {
                // Nobody paid, so there is no one to compensate: return each
                // slash to the member it was taken from instead of leaving
                // it stranded in the contract.
                for (i, w) in (0_u32..).zip(slashed_who.iter()) {
                    let amount = slashed_amt.get_unchecked(i);
                    if amount > 0 {
                        tok.transfer(&this, &w, &amount);
                    }
                }
            }
        }
    }

    // 3) Complete when no active member is still owed a payout; otherwise
    //    advance the cycle.
    let mut done = true;
    for m in payout_order.iter() {
        let st = crate::storage::read_member(&env, circle_id, &m);
        if !st.defaulted && !st.received {
            done = false;
            break;
        }
    }
    if done {
        circle.status = CircleStatus::Completed;
        circle.ended_at = now;
        for m in payout_order.iter() {
            let mut st = crate::storage::read_member(&env, circle_id, &m);
            if !st.defaulted {
                if st.deposit_remaining > 0 {
                    tok.transfer(&this, &m, &st.deposit_remaining);
                    st.deposit_remaining = 0;
                    crate::storage::write_member(&env, circle_id, &m, &st);
                }
                rep.report_completion(&this, &m, &(circle_id as u64));
            }
        }
        crate::events::complete(&env, circle_id);
    } else {
        circle.current_cycle = cycle + 1;
    }
    crate::storage::write_circle(&env, circle_id, &circle);
}
