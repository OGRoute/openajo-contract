#![no_std]
//! OpenAjo `reputation` — permanent cross-circle history per address.
//!
//! Authorized reporter contracts (the `circle` contract) record completions
//! and defaults. Anyone can read. Deposits price default within one circle;
//! this contract is what makes repeat defaulting visible across circles.

mod errors;
mod events;
mod types;

use errors::Error;
use soroban_sdk::{contract, contractimpl, panic_with_error, Address, Env};
use types::{DataKey, Reputation};

/// ~15 days / ~30 days at 5s ledgers.
const TTL_THRESHOLD: u32 = 259_200;
const TTL_EXTEND: u32 = 518_400;

fn extend_instance(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(TTL_THRESHOLD, TTL_EXTEND);
}

#[contract]
pub struct ReputationContract;

#[contractimpl]
impl ReputationContract {
    /// Callable once by anyone; sets the admin who manages reporters.
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic_with_error!(&env, Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        extend_instance(&env);
    }

    /// Admin only: allow or revoke a contract address as an outcome reporter.
    pub fn set_reporter(env: Env, admin: Address, reporter: Address, allowed: bool) {
        admin.require_auth();
        let stored: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_with_error!(&env, Error::NotInitialized));
        if stored != admin {
            panic_with_error!(&env, Error::NotAdmin);
        }
        env.storage()
            .instance()
            .set(&DataKey::Reporter(reporter), &allowed);
        extend_instance(&env);
    }

    /// Authorized reporters only: record a cleanly completed circle.
    pub fn report_completion(env: Env, reporter: Address, member: Address) {
        Self::authorize_reporter(&env, &reporter);
        let mut rep = Self::get_reputation(env.clone(), member.clone());
        rep.completed += 1;
        Self::write_rep(&env, &member, &rep);
        events::completion(&env, &member);
    }

    /// Authorized reporters only: record a default.
    pub fn report_default(env: Env, reporter: Address, member: Address) {
        Self::authorize_reporter(&env, &reporter);
        let mut rep = Self::get_reputation(env.clone(), member.clone());
        rep.defaulted += 1;
        Self::write_rep(&env, &member, &rep);
        events::defaulted(&env, &member);
    }

    /// Anyone: a member's cross-circle history; zeroes if unknown.
    pub fn get_reputation(env: Env, member: Address) -> Reputation {
        env.storage()
            .persistent()
            .get(&DataKey::Rep(member))
            .unwrap_or(Reputation {
                completed: 0,
                defaulted: 0,
            })
    }

    /// Invoker auth (the reporter contract authorizes its own call) plus the
    /// on-storage allow-list — both are required, always.
    fn authorize_reporter(env: &Env, reporter: &Address) {
        reporter.require_auth();
        let allowed: bool = env
            .storage()
            .instance()
            .get(&DataKey::Reporter(reporter.clone()))
            .unwrap_or(false);
        if !allowed {
            panic_with_error!(env, Error::NotReporter);
        }
    }

    fn write_rep(env: &Env, member: &Address, rep: &Reputation) {
        let key = DataKey::Rep(member.clone());
        env.storage().persistent().set(&key, rep);
        env.storage()
            .persistent()
            .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND);
    }
}

mod test;
