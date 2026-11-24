#![no_std]
//! OpenAjo `reputation` — permanent cross-circle history per address.
//!
//! Authorized reporter contracts (the `circle` contract) record completions
//! and defaults, each tagged with the reporting circle's id as `context`.
//! Anyone can read. Deposits price default within one circle; this contract is
//! what makes repeat defaulting visible across circles.
//!
//! Records use the long TTL window and every read refreshes it, but Soroban can
//! still archive an entry nobody touches for ~175 days. Archived history is
//! retained by the network, not deleted: a client restores the `Rep(member)`
//! ledger entry before reading. See `docs/design/reputation.md`.

mod errors;
mod events;
mod types;

use errors::Error;
use soroban_sdk::{contract, contractimpl, panic_with_error, Address, Env, String};
use ttl_helpers::{extend_instance, extend_persistent, extend_persistent_long};
use types::{DataKey, Reputation};

/// Maximum number of members accepted by `get_reputations`.
const MAX_BATCH: u32 = 50;

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
        events::init(&env, &admin);
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
        let key = DataKey::Reporter(reporter.clone());
        env.storage().persistent().set(&key, &allowed);
        extend_persistent(&env, &key);
        events::reporter_set(&env, &reporter, allowed);
    }

    /// Authorized reporters only: record a cleanly completed circle.
    /// `context` is the reporting circle's id (0 when a reporter has none).
    pub fn report_completion(env: Env, reporter: Address, member: Address, context: u64) {
        Self::authorize_reporter(&env, &reporter);
        let mut rep = Self::get_reputation(env.clone(), member.clone());
        rep.completed += 1;
        Self::write_rep(&env, &member, &rep);
        events::completion(&env, &member, context);
    }

    /// Authorized reporters only: record a default.
    /// `context` is the reporting circle's id (0 when a reporter has none).
    pub fn report_default(env: Env, reporter: Address, member: Address, context: u64) {
        Self::authorize_reporter(&env, &reporter);
        let mut rep = Self::get_reputation(env.clone(), member.clone());
        rep.defaulted += 1;
        Self::write_rep(&env, &member, &rep);
        events::defaulted(&env, &member, context);
    }

    /// Anyone: a member's cross-circle history; zeroes if unknown.
    ///
    /// A read of an existing record also refreshes its long TTL, so active
    /// readers keep history live. A read-only simulation does not persist that
    /// refresh (it is not a transaction); if the entry has already been
    /// archived, the caller must restore it first — see the design docs.
    pub fn get_reputation(env: Env, member: Address) -> Reputation {
        let key = DataKey::Rep(member);
        match env.storage().persistent().get(&key) {
            Some(rep) => {
                extend_persistent_long(&env, &key);
                rep
            }
            None => Reputation {
                completed: 0,
                defaulted: 0,
            },
        }
    }

    /// Anyone: batch reputation lookup; order is preserved and unknown members
    /// read as zeroes. Panics `BatchTooLarge` if `members` exceeds `MAX_BATCH`.
    pub fn get_reputations(env: Env, members: Vec<Address>) -> Vec<Reputation> {
        if members.len() > MAX_BATCH {
            panic_with_error!(&env, Error::BatchTooLarge);
        }
        let mut out = Vec::new(&env);
        for member in members.iter() {
            out.push_back(Self::get_reputation(env.clone(), member));
        }
        out
    }

    /// Anyone: returns `true` if `address` is currently an authorized reporter.
    pub fn is_reporter(env: Env, address: Address) -> bool {
        let key = DataKey::Reporter(address);
        env.storage().persistent().get(&key).unwrap_or(false)
    }

    /// Anyone: returns the admin address. Panics `NotInitialized` before setup.
    pub fn get_admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_with_error!(&env, Error::NotInitialized))
    }

    /// Invoker auth (the reporter contract authorizes its own call) plus the
    /// on-storage allow-list — both are required, always.
    fn authorize_reporter(env: &Env, reporter: &Address) {
        reporter.require_auth();
        let key = DataKey::Reporter(reporter.clone());
        let allowed: bool = env.storage().persistent().get(&key).unwrap_or(false);
        if !allowed {
            panic_with_error!(env, Error::NotReporter);
        }
        // An allow-listed reporter that keeps reporting stays live; a revoked
        // or unused flag is allowed to expire.
        extend_persistent(env, &key);
    }

    fn write_rep(env: &Env, member: &Address, rep: &Reputation) {
        let key = DataKey::Rep(member.clone());
        env.storage().persistent().set(&key, rep);
        extend_persistent_long(env, &key);
    }

    /// Returns the CARGO_PKG_VERSION of this build as a string.
    pub fn version(env: Env) -> String {
        String::from_str(&env, env!("CARGO_PKG_VERSION"))
    }
}

mod test;
