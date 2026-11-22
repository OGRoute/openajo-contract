//! Parameter validation helpers for the circle contract.

use soroban_sdk::{panic_with_error, Env};

use crate::errors::Error;
use crate::types::{Circle, CircleStatus};

/// Validate circle creation parameters.
pub fn validate_create_params(
    contribution: i128,
    deposit: i128,
    size: u32,
    period_secs: u64,
    env: &Env,
) {
    if contribution <= 0 || deposit < 0 || size < 2 || period_secs < 3600 {
        panic_with_error!(env, Error::BadParams);
    }
}

/// Validate that a circle is in Open status.
pub fn validate_circle_open(circle: &Circle, env: &Env) {
    if circle.status != CircleStatus::Open {
        panic_with_error!(env, Error::BadStatus);
    }
}

/// Validate that a circle is in Active status.
pub fn validate_circle_active(circle: &Circle, env: &Env) {
    if circle.status != CircleStatus::Active {
        panic_with_error!(env, Error::BadStatus);
    }
}

/// Validate that the caller is the circle creator.
pub fn validate_is_creator(circle: &Circle, caller: &soroban_sdk::Address, env: &Env) {
    if *caller != circle.creator {
        panic_with_error!(env, Error::NotCreator);
    }
}

/// Validate that the caller is not the circle creator.
pub fn validate_not_creator(circle: &Circle, caller: &soroban_sdk::Address, env: &Env) {
    if *caller == circle.creator {
        panic_with_error!(env, Error::IsCreator);
    }
}

/// Validate that a member is not already in the circle.
pub fn validate_not_member(
    members: &soroban_sdk::Vec<soroban_sdk::Address>,
    member: &soroban_sdk::Address,
    env: &Env,
) {
    if members.contains(member) {
        panic_with_error!(env, Error::AlreadyMember);
    }
}

/// Validate that the circle is not full.
pub fn validate_not_full(members: &soroban_sdk::Vec<soroban_sdk::Address>, size: u32, env: &Env) {
    if members.len() >= size {
        panic_with_error!(env, Error::CircleFull);
    }
}

/// Validate that a member has not defaulted.
pub fn validate_not_defaulted(state: &crate::types::MemberState, env: &Env) {
    if state.defaulted {
        panic_with_error!(env, Error::Defaulted);
    }
}

/// Validate that a member has not already paid for this cycle.
pub fn validate_not_paid(paid: bool, env: &Env) {
    if paid {
        panic_with_error!(env, Error::AlreadyPaid);
    }
}

/// Validate that a cycle is due for settlement.
pub fn validate_cycle_due(all_paid: bool, now: u64, deadline: u64, env: &Env) {
    if !all_paid && now <= deadline {
        panic_with_error!(env, Error::NotDue);
    }
}
