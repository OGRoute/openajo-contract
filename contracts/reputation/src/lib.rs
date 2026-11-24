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
//! retained by the network, not deleted: a client restores the `RepV2(member)`
//! ledger entry before reading. See `docs/design/reputation.md`.

mod errors;
mod events;
mod types;

use errors::Error;
use soroban_sdk::{contract, contractimpl, panic_with_error, Address, Bytes, Env, Executable, String};
use ttl_helpers::{extend_instance, extend_persistent, extend_persistent_long};
use types::{DataKey, Reputation, ReputationV1};

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
    ///
    /// `reporter` must be a contract address (Wasm or Stellar Asset Contract),
    /// not a plain account address. Reporters are trusted intermediaries; only
    /// contracts can invoke the report functions and provide the required auth.
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
        // Reject plain Stellar account addresses — they cannot invoke contracts.
        if matches!(reporter.executable(), Some(Executable::Account)) {
            panic_with_error!(&env, Error::NotContractAddress);
        }
        let key = DataKey::Reporter(reporter);
        env.storage().persistent().set(&key, &allowed);
        extend_persistent(&env, &key);
    }

    /// Authorized reporters only: record a cleanly completed circle.
    /// `context` is the reporting circle's id (0 when a reporter has none).
    pub fn report_completion(env: Env, reporter: Address, member: Address, context: u64) {
        Self::authorize_reporter(&env, &reporter);
        let mut rep = Self::get_reputation(env.clone(), member.clone());
        rep.completed += 1;
        rep.last_completed_at = Some(env.ledger().timestamp());
        Self::write_rep(&env, &member, &rep);
        events::completion(&env, &member, context);
    }

    /// Authorized reporters only: record a default.
    /// `context` is the reporting circle's id (0 when a reporter has none).
    pub fn report_default(env: Env, reporter: Address, member: Address, context: u64) {
        Self::authorize_reporter(&env, &reporter);
        let mut rep = Self::get_reputation(env.clone(), member.clone());
        rep.defaulted += 1;
        rep.last_defaulted_at = Some(env.ledger().timestamp());
        Self::write_rep(&env, &member, &rep);
        events::defaulted(&env, &member, context);
    }

    /// Admin only: overwrite a member's counters to correct a reporting error.
    ///
    /// `reason_hash` is an arbitrary bytes payload (e.g. SHA-256 of an
    /// off-chain justification document). The before/after state is emitted in
    /// the event so the correction is auditable on-chain.
    pub fn correct(
        env: Env,
        member: Address,
        completed: u32,
        defaulted: u32,
        reason_hash: Bytes,
    ) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_with_error!(&env, Error::NotInitialized));
        admin.require_auth();
        let before = Self::get_reputation(env.clone(), member.clone());
        let after = Reputation {
            completed,
            defaulted,
            last_completed_at: before.last_completed_at,
            last_defaulted_at: before.last_defaulted_at,
        };
        Self::write_rep(&env, &member, &after);
        events::correction(&env, &member, &before, &after, &reason_hash);
    }

    /// Anyone: a member's normalized reputation score in basis points (0–10 000).
    ///
    /// Returns `completed * 10_000 / (completed + defaulted)`.
    /// Returns 0 for a member with no history.
    pub fn score(env: Env, member: Address) -> u32 {
        let rep = Self::get_reputation(env, member);
        let total = (rep.completed as u64) + (rep.defaulted as u64);
        if total == 0 {
            return 0;
        }
        ((rep.completed as u64) * 10_000 / total) as u32
    }

    /// Anyone: a member's cross-circle history; zeroes if unknown.
    ///
    /// A read of an existing record also refreshes its long TTL, so active
    /// readers keep history live. A read-only simulation does not persist that
    /// refresh (it is not a transaction); if the entry has already been
    /// archived, the caller must restore it first — see the design docs.
    ///
    /// Transparently migrates a legacy `Rep` (v1, no timestamps) entry to
    /// `RepV2` on first read.
    pub fn get_reputation(env: Env, member: Address) -> Reputation {
        let v2_key = DataKey::RepV2(member.clone());
        if let Some(rep) = env.storage().persistent().get::<_, Reputation>(&v2_key) {
            extend_persistent_long(&env, &v2_key);
            return rep;
        }
        // Migrate from v1 if present.
        let v1_key = DataKey::Rep(member.clone());
        if let Some(v1) = env.storage().persistent().get::<_, ReputationV1>(&v1_key) {
            let rep: Reputation = v1.into();
            env.storage().persistent().set(&v2_key, &rep);
            extend_persistent_long(&env, &v2_key);
            // Leave the v1 entry in place so an in-flight restore still works;
            // it will expire naturally when its TTL is not refreshed.
            return rep;
        }
        Reputation {
            completed: 0,
            defaulted: 0,
            last_completed_at: None,
            last_defaulted_at: None,
        }
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
        let key = DataKey::RepV2(member.clone());
        env.storage().persistent().set(&key, rep);
        extend_persistent_long(env, &key);
    }

    /// Returns the CARGO_PKG_VERSION of this build as a string.
    pub fn version(env: Env) -> String {
        String::from_str(&env, env!("CARGO_PKG_VERSION"))
    }
}

mod test;
