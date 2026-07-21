#![no_std]
//! OpenAjo `circle` — the full ROSCA (ajo/esusu/adashe) lifecycle.
//!
//! One instance manages many circles. A circle escrows a flat deposit from
//! each member, collects a fixed contribution per cycle, and pays the whole
//! pot to each member in join order, one cycle at a time. Missed contributions
//! are slashed from the member's deposit; a deposit that cannot cover a miss
//! marks the member defaulted (skipped in rotation, reported to `reputation`).

mod errors;
mod events;
mod types;

use core::cmp::min;

use errors::Error;
use soroban_sdk::{
    contract, contractclient, contractimpl, panic_with_error, token, vec, Address, Env, Vec,
};
use types::{Circle, CircleStatus, DataKey, GlobalConfig, MemberState};

/// ~15 days / ~30 days at 5s ledgers.
const TTL_THRESHOLD: u32 = 259_200;
const TTL_EXTEND: u32 = 518_400;

/// Minimal interface of the `reputation` contract (same workspace).
#[contractclient(name = "ReputationClient")]
pub trait ReputationIface {
    fn report_completion(env: Env, reporter: Address, member: Address);
    fn report_default(env: Env, reporter: Address, member: Address);
}

#[contract]
pub struct CircleContract;

#[contractimpl]
impl CircleContract {
    /// Callable once by anyone; stores admin and the reputation contract.
    pub fn initialize(env: Env, admin: Address, reputation: Address) {
        if env.storage().instance().has(&DataKey::Config) {
            panic_with_error!(&env, Error::AlreadyInitialized);
        }
        env.storage()
            .instance()
            .set(&DataKey::Config, &GlobalConfig { admin, reputation });
        env.storage().instance().set(&DataKey::Count, &0u32);
        extend_instance(&env);
    }

    /// Creator only: open a new circle and escrow the creator's deposit.
    pub fn create_circle(
        env: Env,
        creator: Address,
        token: Address,
        contribution: i128,
        deposit: i128,
        size: u32,
        period_secs: u64,
    ) -> u32 {
        creator.require_auth();
        if contribution <= 0 || deposit < 0 || size < 2 || period_secs < 3600 {
            panic_with_error!(&env, Error::BadParams);
        }
        let id: u32 = env.storage().instance().get(&DataKey::Count).unwrap_or(0);

        if deposit > 0 {
            token::Client::new(&env, &token).transfer(
                &creator,
                &env.current_contract_address(),
                &deposit,
            );
        }

        let circle = Circle {
            creator: creator.clone(),
            token,
            contribution,
            deposit,
            size,
            period_secs,
            status: CircleStatus::Open,
            started_at: 0,
            current_cycle: 0,
        };
        write_circle(&env, id, &circle);
        write_members(&env, id, &vec![&env, creator.clone()]);
        write_member(
            &env,
            id,
            &creator,
            &MemberState {
                deposit_remaining: deposit,
                received: false,
                defaulted: false,
            },
        );
        env.storage().instance().set(&DataKey::Count, &(id + 1));
        extend_instance(&env);
        events::create(&env, id, &creator);
        id
    }

    /// Member only: join an Open circle, escrowing the deposit. Activates the
    /// circle when it reaches `size`.
    pub fn join(env: Env, circle_id: u32, member: Address) {
        member.require_auth();
        let mut circle = read_circle(&env, circle_id);
        if circle.status != CircleStatus::Open {
            panic_with_error!(&env, Error::BadStatus);
        }
        let mut members = read_members(&env, circle_id);
        if members.contains(&member) {
            panic_with_error!(&env, Error::AlreadyMember);
        }
        if members.len() >= circle.size {
            panic_with_error!(&env, Error::CircleFull);
        }

        if circle.deposit > 0 {
            token::Client::new(&env, &circle.token).transfer(
                &member,
                &env.current_contract_address(),
                &circle.deposit,
            );
        }
        members.push_back(member.clone());
        write_members(&env, circle_id, &members);
        write_member(
            &env,
            circle_id,
            &member,
            &MemberState {
                deposit_remaining: circle.deposit,
                received: false,
                defaulted: false,
            },
        );
        events::join(&env, circle_id, &member);

        if members.len() == circle.size {
            circle.status = CircleStatus::Active;
            circle.started_at = env.ledger().timestamp();
            events::start(&env, circle_id);
        }
        write_circle(&env, circle_id, &circle);
    }

    /// Member only, while Open, not the creator: exit and reclaim the deposit.
    pub fn leave(env: Env, circle_id: u32, member: Address) {
        member.require_auth();
        let circle = read_circle(&env, circle_id);
        if circle.status != CircleStatus::Open {
            panic_with_error!(&env, Error::BadStatus);
        }
        if member == circle.creator {
            panic_with_error!(&env, Error::IsCreator);
        }
        let members = read_members(&env, circle_id);
        let state = read_member(&env, circle_id, &member);

        if state.deposit_remaining > 0 {
            token::Client::new(&env, &circle.token).transfer(
                &env.current_contract_address(),
                &member,
                &state.deposit_remaining,
            );
        }
        let mut remaining: Vec<Address> = vec![&env];
        for m in members.iter() {
            if m != member {
                remaining.push_back(m);
            }
        }
        write_members(&env, circle_id, &remaining);
        env.storage()
            .persistent()
            .remove(&DataKey::Member(circle_id, member));
    }

    /// Creator only, while Open: cancel and refund every member's deposit.
    pub fn cancel(env: Env, circle_id: u32, creator: Address) {
        creator.require_auth();
        let mut circle = read_circle(&env, circle_id);
        if creator != circle.creator {
            panic_with_error!(&env, Error::NotCreator);
        }
        if circle.status != CircleStatus::Open {
            panic_with_error!(&env, Error::BadStatus);
        }
        let members = read_members(&env, circle_id);
        let tok = token::Client::new(&env, &circle.token);
        for m in members.iter() {
            let state = read_member(&env, circle_id, &m);
            if state.deposit_remaining > 0 {
                tok.transfer(
                    &env.current_contract_address(),
                    &m,
                    &state.deposit_remaining,
                );
            }
        }
        circle.status = CircleStatus::Cancelled;
        write_circle(&env, circle_id, &circle);
        events::cancel(&env, circle_id);
    }

    /// Member only, while Active: pay this cycle's contribution.
    pub fn contribute(env: Env, circle_id: u32, member: Address) {
        member.require_auth();
        let circle = read_circle(&env, circle_id);
        if circle.status != CircleStatus::Active {
            panic_with_error!(&env, Error::BadStatus);
        }
        let state = read_member(&env, circle_id, &member);
        if state.defaulted {
            panic_with_error!(&env, Error::Defaulted);
        }
        let key = DataKey::Paid(circle_id, circle.current_cycle, member.clone());
        if env.storage().temporary().get(&key).unwrap_or(false) {
            panic_with_error!(&env, Error::AlreadyPaid);
        }
        token::Client::new(&env, &circle.token).transfer(
            &member,
            &env.current_contract_address(),
            &circle.contribution,
        );
        env.storage().temporary().set(&key, &true);
        events::contrib(&env, circle_id, &member, circle.current_cycle);
    }

    /// Permissionless crank: anyone may settle a due cycle. Funds only move
    /// according to protocol rules, so no auth is required — this is what lets
    /// the indexer (or any member) advance a circle whose deadline passed.
    pub fn settle_cycle(env: Env, circle_id: u32) {
        let mut circle = read_circle(&env, circle_id);
        if circle.status != CircleStatus::Active {
            panic_with_error!(&env, Error::BadStatus);
        }
        let members = read_members(&env, circle_id);
        let cycle = circle.current_cycle;
        let now = env.ledger().timestamp();
        let deadline = circle.started_at + (cycle as u64 + 1) * circle.period_secs;

        // Due when every active member has paid, or the deadline has passed.
        let mut all_paid = true;
        for m in members.iter() {
            let st = read_member(&env, circle_id, &m);
            if st.defaulted {
                continue;
            }
            if !is_paid(&env, circle_id, cycle, &m) {
                all_paid = false;
                break;
            }
        }
        if !all_paid && now <= deadline {
            panic_with_error!(&env, Error::NotDue);
        }

        let config: GlobalConfig = read_config(&env);
        let tok = token::Client::new(&env, &circle.token);
        let rep = ReputationClient::new(&env, &config.reputation);
        let this = env.current_contract_address();

        // 1) Collect the pot: contributions already escrowed by payers, plus
        //    slashes from active members who missed.
        let mut pot: i128 = 0;
        let mut payers: Vec<Address> = vec![&env];
        for m in members.iter() {
            let mut st = read_member(&env, circle_id, &m);
            if st.defaulted {
                continue;
            }
            if is_paid(&env, circle_id, cycle, &m) {
                pot += circle.contribution;
                payers.push_back(m.clone());
            } else {
                let slash = min(st.deposit_remaining, circle.contribution);
                if slash > 0 {
                    st.deposit_remaining -= slash;
                    pot += slash;
                    events::slash(&env, circle_id, &m, slash);
                }
                if slash < circle.contribution {
                    st.defaulted = true;
                    events::defaulted(&env, circle_id, &m);
                    rep.report_default(&this, &m);
                }
                write_member(&env, circle_id, &m, &st);
            }
        }

        // 2) Pay the first member in join order who is active and unpaid.
        let mut recipient: Option<Address> = None;
        for m in members.iter() {
            let st = read_member(&env, circle_id, &m);
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
                let mut st = read_member(&env, circle_id, &r);
                st.received = true;
                write_member(&env, circle_id, &r, &st);
                events::payout(&env, circle_id, &r, pot, cycle);
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
                }
            }
        }

        // 3) Complete when no active member is still owed a payout; otherwise
        //    advance the cycle.
        let mut done = true;
        for m in members.iter() {
            let st = read_member(&env, circle_id, &m);
            if !st.defaulted && !st.received {
                done = false;
                break;
            }
        }
        if done {
            circle.status = CircleStatus::Completed;
            for m in members.iter() {
                let mut st = read_member(&env, circle_id, &m);
                if !st.defaulted {
                    if st.deposit_remaining > 0 {
                        tok.transfer(&this, &m, &st.deposit_remaining);
                        st.deposit_remaining = 0;
                        write_member(&env, circle_id, &m, &st);
                    }
                    rep.report_completion(&this, &m);
                }
            }
            events::complete(&env, circle_id);
        } else {
            circle.current_cycle = cycle + 1;
        }
        write_circle(&env, circle_id, &circle);
    }

    // ---- Views ------------------------------------------------------------

    pub fn get_circle(env: Env, circle_id: u32) -> Circle {
        read_circle(&env, circle_id)
    }

    pub fn get_members(env: Env, circle_id: u32) -> Vec<Address> {
        read_members(&env, circle_id)
    }

    pub fn get_member(env: Env, circle_id: u32, member: Address) -> MemberState {
        read_member(&env, circle_id, &member)
    }

    pub fn cycle_deadline(env: Env, circle_id: u32) -> u64 {
        let circle = read_circle(&env, circle_id);
        circle.started_at + (circle.current_cycle as u64 + 1) * circle.period_secs
    }

    pub fn total_circles(env: Env) -> u32 {
        env.storage().instance().get(&DataKey::Count).unwrap_or(0)
    }
}

// ---- Storage helpers ------------------------------------------------------

fn extend_instance(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(TTL_THRESHOLD, TTL_EXTEND);
}

fn read_config(env: &Env) -> GlobalConfig {
    env.storage()
        .instance()
        .get(&DataKey::Config)
        .unwrap_or_else(|| panic_with_error!(env, Error::NotInitialized))
}

fn read_circle(env: &Env, id: u32) -> Circle {
    env.storage()
        .persistent()
        .get(&DataKey::Circle(id))
        .unwrap_or_else(|| panic_with_error!(env, Error::NotFound))
}

fn write_circle(env: &Env, id: u32, circle: &Circle) {
    let key = DataKey::Circle(id);
    env.storage().persistent().set(&key, circle);
    env.storage()
        .persistent()
        .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND);
}

fn read_members(env: &Env, id: u32) -> Vec<Address> {
    env.storage()
        .persistent()
        .get(&DataKey::Members(id))
        .unwrap_or_else(|| panic_with_error!(env, Error::NotFound))
}

fn write_members(env: &Env, id: u32, members: &Vec<Address>) {
    let key = DataKey::Members(id);
    env.storage().persistent().set(&key, members);
    env.storage()
        .persistent()
        .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND);
}

fn read_member(env: &Env, id: u32, member: &Address) -> MemberState {
    env.storage()
        .persistent()
        .get(&DataKey::Member(id, member.clone()))
        .unwrap_or_else(|| panic_with_error!(env, Error::NotMember))
}

fn write_member(env: &Env, id: u32, member: &Address, state: &MemberState) {
    let key = DataKey::Member(id, member.clone());
    env.storage().persistent().set(&key, state);
    env.storage()
        .persistent()
        .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND);
}

fn is_paid(env: &Env, id: u32, cycle: u32, member: &Address) -> bool {
    env.storage()
        .temporary()
        .get(&DataKey::Paid(id, cycle, member.clone()))
        .unwrap_or(false)
}

mod test;
