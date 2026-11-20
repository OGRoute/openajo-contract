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
mod settlement;
mod storage;
mod types;
mod validation;

use errors::Error;
use soroban_sdk::{
    contract, contractclient, contractimpl, panic_with_error, token, vec, Address, Env, Vec,
};
use ttl_helpers::extend_instance;
use types::{Circle, CircleStatus, DataKey, GlobalConfig, MemberState};

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
        validation::validate_create_params(contribution, deposit, size, period_secs, &env);
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
            events::start(&env, circle_id);
        }
        storage::write_circle(&env, circle_id, &circle);
    }

    /// Member only, while Open, not the creator: exit and reclaim the deposit.
    pub fn leave(env: Env, circle_id: u32, member: Address) {
        member.require_auth();
        let circle = storage::read_circle(&env, circle_id);
        validation::validate_circle_open(&circle, &env);
        validation::validate_not_creator(&circle, &member, &env);
        let members = storage::read_members(&env, circle_id);
        let state = storage::read_member(&env, circle_id, &member);

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
        storage::write_members(&env, circle_id, &remaining);
        env.storage()
            .persistent()
            .remove(&DataKey::Member(circle_id, member));
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
    }

    /// Permissionless crank: anyone may settle a due cycle. Funds only move
    /// according to protocol rules, so no auth is required — this is what lets
    /// the indexer (or any member) advance a circle whose deadline passed.
    pub fn settle_cycle(env: Env, circle_id: u32) {
        settlement::settle_cycle(env, circle_id);
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
}

mod test;

mod test_invariants;

mod test_reputation;

mod test_cost;
