//! Settlement logic for the circle contract.

use core::cmp::min;

use soroban_sdk::token;
use soroban_sdk::{panic_with_error, vec, Address, Env, Vec};

use crate::errors::Error;
use crate::types::{CircleStatus, DataKey, GlobalConfig};
use crate::ReputationClient;

/// Settle a due cycle, optionally rewarding `keeper` from slashed funds.
///
/// `keeper` is any address the caller supplies; if the circle's
/// `keeper_reward_bps` is non-zero and there are slashes this cycle, `keeper`
/// receives that fraction of the slashed amount before the pot is paid out.
/// No auth is required from `keeper`.
pub fn settle_cycle(env: Env, circle_id: u32, keeper: Address) {
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
    //    slashes from active members who missed. Track slashed_total before
    //    this cycle so we can compute the keeper's share.
    let old_slashed_total = circle.slashed_total;
    let mut pot: i128 = 0;
    let mut payers: Vec<Address> = vec![&env];
    let mut slashed_who: Vec<Address> = vec![&env];
    let mut slashed_amt: Vec<i128> = vec![&env];
    let mut cycle_slashed: i128 = 0;
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
                circle.slashed_total += slash;
                cycle_slashed += slash;
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

    // Delete the Paid flags for this cycle; they are never read again after
    // settlement and removing them frees temporary-storage ledger entries.
    for p in payers.iter() {
        env.storage()
            .temporary()
            .remove(&DataKey::Paid(circle_id, cycle, p));
    }

    // 2) Compute keeper reward from this cycle's slashes only — never from
    //    contributions. The reward is deducted from the pot before payout.
    let slash_this_cycle = circle.slashed_total - old_slashed_total;
    let keeper_reward = (slash_this_cycle * circle.keeper_reward_bps as i128) / 10_000;

    // 3) Find the first recipient in payout order: custom order if set,
    //    otherwise join order (the members list).
    let custom_order = crate::storage::read_payout_order(&env, circle_id);
    let order: &Vec<Address> = custom_order.as_ref().unwrap_or(&members);
    let mut recipient: Option<Address> = None;
    for m in order.iter() {
        let st = crate::storage::read_member(&env, circle_id, &m);
        if !st.received && !st.defaulted {
            recipient = Some(m);
            break;
        }
    }
    let settle_recipient = recipient.clone();
    match recipient {
        Some(r) => {
            // Pay keeper from slash proceeds (never from contributions).
            if keeper_reward > 0 {
                tok.transfer(&this, &keeper, &keeper_reward);
                crate::events::keeper_reward(&env, circle_id, &keeper, keeper_reward);
            }
            let net_pot = pot - keeper_reward;
            if net_pot > 0 {
                tok.transfer(&this, &r, &net_pot);
            }
            let mut st = crate::storage::read_member(&env, circle_id, &r);
            st.received = true;
            crate::storage::write_member(&env, circle_id, &r, &st);
            crate::events::payout(&env, circle_id, &r, net_pot, cycle);
        }
        None => {
            // Edge: this settlement's defaults eliminated every remaining
            // unpaid member. Return contributions to payers and split the
            // slashed amount equally among them (remainder to the first),
            // so no funds strand in the contract. Keeper receives nothing
            // when there is no recipient.
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

    // Emit a single summary event for the settled cycle.
    crate::events::settle(
        &env,
        circle_id,
        cycle,
        &payers,
        cycle_slashed,
        settle_recipient,
    );

    // 4) Complete when no active member is still owed a payout; otherwise
    //    advance the cycle.
    let mut done = true;
    for m in members.iter() {
        let st = crate::storage::read_member(&env, circle_id, &m);
        if !st.defaulted && !st.received {
            done = false;
            break;
        }
    }
    if done {
        circle.status = CircleStatus::Completed;
        circle.ended_at = now;
        for m in members.iter() {
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
