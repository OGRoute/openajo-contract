#![no_std]
#![allow(clippy::too_many_arguments)]
//! OpenAjo `circle` — the full ROSCA (ajo/esusu/adashe) lifecycle.
//!
//! One instance manages many circles. A circle escrows a flat deposit from
//! each member, collects a fixed contribution per cycle, and pays the whole
//! pot to each member in join order, one cycle at a time. Missed contributions
//! are slashed from the member's deposit; a deposit that cannot cover a miss
//! marks the member defaulted (skipped in rotation, reported to `reputation`).

#[cfg(test)]
extern crate std;

mod errors;
mod events;
mod settlement;
mod storage;
mod types;
mod validation;

use errors::Error;
use soroban_sdk::{
    contract, contractclient, contractimpl, panic_with_error, token, vec, Address, Env, String, Vec,
};
use ttl_helpers::{extend_instance, extend_persistent};
use types::{Circle, CircleStatus, DataKey, GlobalConfig, MemberState};

/// Maximum number of circles returned by `get_circles` per call.
pub const MAX_PAGE_SIZE: u32 = 50;

/// Shortest cycle a circle may run, in seconds (one hour).
///
/// `create_circle` reverts with `BadParams` when `period_secs` is below this.
/// The settlement deadline of every cycle is `started_at + (cycle + 1) *
/// period_secs`, so this is also the shortest time members get to contribute.
pub const MIN_PERIOD_SECS: u64 = 3600;

/// Minimal interface of the `reputation` contract (same workspace).
/// `context` is this circle's id, recorded in the reputation event so an
/// outcome can be traced back to the circle that produced it.
#[contractclient(name = "ReputationClient")]
pub trait ReputationIface {
    fn report_completion(env: Env, reporter: Address, member: Address, context: u64);
    fn report_default(env: Env, reporter: Address, member: Address, context: u64);
}

#[contract]
pub struct CircleContract;

#[contractimpl]
#[allow(clippy::too_many_arguments)]
impl CircleContract {
    /// Callable once by anyone; stores admin and the reputation contract.
    pub fn initialize(env: Env, admin: Address, reputation: Address) {
        if env.storage().instance().has(&DataKey::Config) {
            panic_with_error!(&env, Error::AlreadyInitialized);
        }
        env.storage().instance().set(
            &DataKey::Config,
            &GlobalConfig {
                admin: admin.clone(),
                reputation: reputation.clone(),
                open_circle_cap: u32::MAX,
            },
        );
        env.storage().instance().set(&DataKey::Count, &0u32);
        extend_instance(&env);
        events::init(&env, &admin, &reputation);
    }

    /// Admin only: sets the cap on the number of `Open` circles a single creator
    /// may have at once. `u32::MAX` means no cap (the initialization default).
    /// Requires `config.admin` auth.
    pub fn set_open_circle_cap(env: Env, cap: u32) {
        let mut config: GlobalConfig = storage::read_config(&env);
        config.admin.require_auth();
        config.open_circle_cap = cap;
        env.storage().instance().set(&DataKey::Config, &config);
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
        auto_settle: bool,
        randomize_order: bool,
        keeper_reward_bps: u32,
    ) -> u32 {
        creator.require_auth();
        validation::validate_create_params(contribution, deposit, size, period_secs, &env);

        let config: GlobalConfig = storage::read_config(&env);
        let open_key = DataKey::OpenCircles(creator.clone());
        let open_count: u32 = env.storage().persistent().get(&open_key).unwrap_or(0);
        if open_count >= config.open_circle_cap {
            panic_with_error!(&env, Error::OpenCircleCapExceeded);
        }

        let id: u32 = env.storage().instance().get(&DataKey::Count).unwrap_or(0);

        if deposit > 0 {
            token::Client::new(&env, &token).transfer(
                &creator,
                env.current_contract_address(),
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
            slashed_total: 0,
            ended_at: 0,
            auto_settle,
            randomize_order,
            keeper_reward_bps,
        };
        storage::write_circle(&env, id, &circle);
        storage::write_members(&env, id, &vec![&env, creator.clone()]);
        storage::write_member(
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
        env.storage().persistent().set(&open_key, &(open_count + 1));
        extend_persistent(&env, &open_key);
        extend_instance(&env);
        events::create(&env, id, &creator);
        id
    }

    /// Member only: join an Open circle, escrowing the deposit. Activates the
    /// circle when it reaches `size`.
    pub fn join(env: Env, circle_id: u32, member: Address) {
        member.require_auth();
        let mut circle = storage::read_circle(&env, circle_id);
        validation::validate_circle_open(&circle, &env);
        let mut members = storage::read_members(&env, circle_id);
        validation::validate_not_member(&members, &member, &env);
        validation::validate_not_full(&members, circle.size, &env);

        if circle.deposit > 0 {
            token::Client::new(&env, &circle.token).transfer(
                &member,
                env.current_contract_address(),
                &circle.deposit,
            );
        }
        members.push_back(member.clone());
        storage::write_members(&env, circle_id, &members);
        storage::write_member(
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
            // Randomise payout order at activation if requested.
            if circle.randomize_order {
                let n = members.len();
                let mut i = n;
                while i > 1 {
                    i -= 1;
                    let j = env.prng().gen_range::<u64>(0..=i as u64) as u32;
                    let a = members.get_unchecked(i);
                    let b = members.get_unchecked(j);
                    members.set(i, b);
                    members.set(j, a);
                }
                storage::write_members(&env, circle_id, &members);
            }
            events::start(&env, circle_id);
            // Circle activated: decrement creator's open circle count.
            let open_key = DataKey::OpenCircles(circle.creator.clone());
            let open_count: u32 = env.storage().persistent().get(&open_key).unwrap_or(0);
            if open_count > 0 {
                env.storage().persistent().set(&open_key, &(open_count - 1));
                extend_persistent(&env, &open_key);
            }
        }
        storage::write_circle(&env, circle_id, &circle);
    }

    /// Member only, while Open, not the creator: exit and reclaim the deposit.
    pub fn leave(env: Env, circle_id: u32, member: Address) {
        member.require_auth();
        let circle = storage::read_circle(&env, circle_id);
        validation::validate_circle_open(&circle, &env);
        validation::validate_not_creator(&circle, &member, &env);
        let mut members = storage::read_members(&env, circle_id);
        let state = storage::read_member(&env, circle_id, &member);

        if state.deposit_remaining > 0 {
            token::Client::new(&env, &circle.token).transfer(
                &env.current_contract_address(),
                &member,
                &state.deposit_remaining,
            );
        }
        // `read_member` above already rejects a non-member. `remove` shifts the
        // later members down, so join order, which is payout order, is kept.
        if let Some(index) = members.first_index_of(&member) {
            members.remove(index);
        }
        storage::write_members(&env, circle_id, &members);
        env.storage()
            .persistent()
            .remove(&DataKey::Member(circle_id, member));
    }

    /// Creator only, while Open: remove a member and refund their deposit.
    /// The remaining members keep their join order, so payout order is
    /// unchanged. The creator cannot be removed; they cancel instead.
    pub fn remove_member(env: Env, circle_id: u32, creator: Address, member: Address) {
        creator.require_auth();
        let circle = storage::read_circle(&env, circle_id);
        validation::validate_is_creator(&circle, &creator, &env);
        validation::validate_circle_open(&circle, &env);
        validation::validate_not_creator(&circle, &member, &env);
        let state = storage::read_member(&env, circle_id, &member);
        let mut members = storage::read_members(&env, circle_id);

        if state.deposit_remaining > 0 {
            token::Client::new(&env, &circle.token).transfer(
                &env.current_contract_address(),
                &member,
                &state.deposit_remaining,
            );
        }
        // `remove` shifts the later members down, so join order, which is
        // payout order, is kept. Mirrors `leave`.
        if let Some(index) = members.first_index_of(&member) {
            members.remove(index);
        }
        storage::write_members(&env, circle_id, &members);
        env.storage()
            .persistent()
            .remove(&DataKey::Member(circle_id, member.clone()));
        events::remove(&env, circle_id, &member);
    }

    /// Creator only, while Open: cancel and refund every member's deposit.
    pub fn cancel(env: Env, circle_id: u32, creator: Address) {
        creator.require_auth();
        let mut circle = storage::read_circle(&env, circle_id);
        validation::validate_is_creator(&circle, &creator, &env);
        validation::validate_circle_open(&circle, &env);
        let members = storage::read_members(&env, circle_id);
        let tok = token::Client::new(&env, &circle.token);
        for m in members.iter() {
            let mut state = storage::read_member(&env, circle_id, &m);
            if state.deposit_remaining > 0 {
                tok.transfer(
                    &env.current_contract_address(),
                    &m,
                    &state.deposit_remaining,
                );
                state.deposit_remaining = 0;
                storage::write_member(&env, circle_id, &m, &state);
            }
        }
        circle.status = CircleStatus::Cancelled;
        circle.ended_at = env.ledger().timestamp();
        storage::write_circle(&env, circle_id, &circle);
        events::cancel(&env, circle_id);
        // Circle cancelled: decrement creator's open circle count.
        let open_key = DataKey::OpenCircles(circle.creator.clone());
        let open_count: u32 = env.storage().persistent().get(&open_key).unwrap_or(0);
        if open_count > 0 {
            env.storage().persistent().set(&open_key, &(open_count - 1));
            extend_persistent(&env, &open_key);
        }
    }

    /// Member only, while Active: pay this cycle's contribution.
    pub fn contribute(env: Env, circle_id: u32, member: Address) {
        member.require_auth();
        let circle = storage::read_circle(&env, circle_id);
        validation::validate_circle_active(&circle, &env);
        let state = storage::read_member(&env, circle_id, &member);
        validation::validate_not_defaulted(&state, &env);
        let key = DataKey::Paid(circle_id, circle.current_cycle, member.clone());
        let paid = env.storage().temporary().get(&key).unwrap_or(false);
        validation::validate_not_paid(paid, &env);
        token::Client::new(&env, &circle.token).transfer(
            &member,
            env.current_contract_address(),
            &circle.contribution,
        );
        env.storage().temporary().set(&key, &true);
        events::contrib(&env, circle_id, &member, circle.current_cycle);

        // Auto-settle: if the circle requested it, check whether every active
        // member has now paid and trigger settlement inline if so.
        if circle.auto_settle {
            let members = storage::read_members(&env, circle_id);
            let mut all_active_paid = true;
            for m in members.iter() {
                let st = storage::read_member(&env, circle_id, &m);
                if st.defaulted {
                    continue;
                }
                if !storage::is_paid(&env, circle_id, circle.current_cycle, &m) {
                    all_active_paid = false;
                    break;
                }
            }
            if all_active_paid {
                settlement::settle_cycle(env, circle_id, member);
            }
        }
    }

    /// Permissionless crank: anyone may settle a due cycle. `keeper` is the
    /// address that will receive a keeper reward from any slashes this cycle,
    /// proportional to the circle's `keeper_reward_bps`. No auth is required.
    pub fn settle_cycle(env: Env, circle_id: u32, keeper: Address) {
        settlement::settle_cycle(env, circle_id, keeper);
    }

    /// Creator only, before the first cycle settles: store a custom payout
    /// order that overrides the default join order. Requires auth from every
    /// non-defaulted member so no single party can re-route payouts.
    pub fn set_payout_order(env: Env, circle_id: u32, creator: Address, order: Vec<Address>) {
        creator.require_auth();
        let circle = storage::read_circle(&env, circle_id);
        validation::validate_is_creator(&circle, &creator, &env);
        validation::validate_circle_active(&circle, &env);
        if circle.current_cycle != 0 {
            panic_with_error!(&env, Error::BadStatus);
        }
        let members = storage::read_members(&env, circle_id);
        // Require every non-defaulted member to co-sign.
        for m in members.iter() {
            let st = storage::read_member(&env, circle_id, &m);
            if !st.defaulted {
                m.require_auth();
            }
        }
        // Validate that order is a permutation of the member list.
        if order.len() != members.len() {
            panic_with_error!(&env, Error::BadOrder);
        }
        for m in members.iter() {
            if !order.contains(&m) {
                panic_with_error!(&env, Error::BadOrder);
            }
        }
        storage::write_payout_order(&env, circle_id, &order);
        extend_instance(&env);
        events::order_set(&env, circle_id);
    }

    // ---- Views ------------------------------------------------------------

    pub fn get_circle(env: Env, circle_id: u32) -> Circle {
        storage::read_circle(&env, circle_id)
    }

    pub fn get_members(env: Env, circle_id: u32) -> Vec<Address> {
        storage::read_members(&env, circle_id)
    }

    pub fn get_member(env: Env, circle_id: u32, member: Address) -> MemberState {
        storage::read_member(&env, circle_id, &member)
    }

    pub fn cycle_deadline(env: Env, circle_id: u32) -> u64 {
        let circle = storage::read_circle(&env, circle_id);
        circle.started_at + (circle.current_cycle as u64 + 1) * circle.period_secs
    }

    pub fn total_circles(env: Env) -> u32 {
        env.storage().instance().get(&DataKey::Count).unwrap_or(0)
    }

    // ---- New views (#66 #67 #68 #69) --------------------------------------

    /// Returns the global configuration set at initialization.
    /// Panics `NotInitialized` if called before `initialize`.
    pub fn get_config(env: Env) -> GlobalConfig {
        storage::read_config(&env)
    }

    /// Returns up to `limit` circles starting from `start` (inclusive).
    /// `limit` is capped at [`MAX_PAGE_SIZE`]. Ids past `total_circles` are
    /// silently absent from the result.
    pub fn get_circles(env: Env, start: u32, limit: u32) -> Vec<(u32, Circle)> {
        let total: u32 = env.storage().instance().get(&DataKey::Count).unwrap_or(0);
        let effective_limit = limit.min(MAX_PAGE_SIZE);
        let end = total.min(start.saturating_add(effective_limit));
        let mut result: Vec<(u32, Circle)> = Vec::new(&env);
        for id in start..end {
            let circle = storage::read_circle(&env, id);
            result.push_back((id, circle));
        }
        result
    }

    /// Returns the 0-based cycle in which `member` is due to receive the payout.
    /// Defaulted members ahead of `member` in join order are skipped so their
    /// removed slot does not delay those behind them.
    /// Panics `Defaulted` if `member` itself has defaulted.
    /// Panics `NotMember` if `member` is not in the circle.
    pub fn payout_position(env: Env, circle_id: u32, member: Address) -> u32 {
        let members = storage::read_members(&env, circle_id);
        let member_state = storage::read_member(&env, circle_id, &member);
        if member_state.defaulted {
            panic_with_error!(&env, Error::Defaulted);
        }
        let mut pos = 0u32;
        for m in members.iter() {
            let state = storage::read_member(&env, circle_id, &m);
            if !state.defaulted {
                if m == member {
                    return pos;
                }
                pos += 1;
            }
        }
        panic_with_error!(&env, Error::NotMember)
    }

    /// Returns `true` when `member` is currently in the circle's member list,
    /// `false` for addresses that never joined or that left before activation.
    pub fn is_member(env: Env, circle_id: u32, member: Address) -> bool {
        env.storage()
            .persistent()
            .has(&DataKey::Member(circle_id, member))
    }

    /// Returns `true` when `member` has already contributed to the circle's
    /// current cycle, `false` otherwise (including after `settle_cycle` removes
    /// the flag).
    pub fn has_paid(env: Env, circle_id: u32, member: Address) -> bool {
        let circle = storage::read_circle(&env, circle_id);
        storage::is_paid(&env, circle_id, circle.current_cycle, &member)
    }

    /// Returns the CARGO_PKG_VERSION of this build as a string.
    pub fn version(env: Env) -> String {
        String::from_str(&env, env!("CARGO_PKG_VERSION"))
    }
}

mod test;

#[cfg(test)]
mod test_utils;

mod test_invariants;

mod test_reputation;

mod test_cost;
