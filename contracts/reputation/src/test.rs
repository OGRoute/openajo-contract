#![cfg(test)]

extern crate std;

use crate::{errors::Error, types::Reputation, ReputationContract, ReputationContractClient};
use soroban_sdk::{
    symbol_short,
    testutils::{
        storage::Persistent as _, Address as _, AuthorizedFunction, AuthorizedInvocation, Events,
        Ledger,
    },
    vec, Address, Bytes, Env, IntoVal, Symbol, Val, Vec,
};

/// Circle id used as the report context in tests.
const CTX: u64 = 7;

fn setup() -> (Env, ReputationContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(ReputationContract, ());
    let client = ReputationContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin)
}

/// Returns `Reputation` with both timestamp fields set to `None`.
fn rep(completed: u32, defaulted: u32) -> Reputation {
    Reputation {
        completed,
        defaulted,
        last_completed_at: None,
        last_defaulted_at: None,
    }
}

#[test]
fn reports_increment_counts() {
    let (env, client, admin) = setup();
    let reporter = Address::generate(&env);
    let member = Address::generate(&env);
    client.set_reporter(&admin, &reporter, &true);

    client.report_completion(&reporter, &member, &CTX);
    client.report_completion(&reporter, &member, &CTX);
    client.report_default(&reporter, &member, &CTX);

    let r = client.get_reputation(&member);
    assert_eq!(r.completed, 2);
    assert_eq!(r.defaulted, 1);
}

#[test]
fn unknown_member_reads_zero() {
    let (env, client, _) = setup();
    let stranger = Address::generate(&env);
    assert_eq!(client.get_reputation(&stranger), rep(0, 0));
}

#[test]
#[should_panic]
fn initialize_twice_panics() {
    let (env, client, _) = setup();
    let other = Address::generate(&env);
    client.initialize(&other);
}

#[test]
#[should_panic]
fn unauthorized_reporter_panics() {
    let (env, client, _) = setup();
    let rogue = Address::generate(&env);
    let member = Address::generate(&env);
    client.report_completion(&rogue, &member, &CTX);
}

#[test]
#[should_panic]
fn revoked_reporter_panics() {
    let (env, client, admin) = setup();
    let reporter = Address::generate(&env);
    let member = Address::generate(&env);
    client.set_reporter(&admin, &reporter, &true);
    client.set_reporter(&admin, &reporter, &false);
    client.report_default(&reporter, &member, &CTX);
}

#[test]
#[should_panic]
fn non_admin_set_reporter_panics() {
    let (env, client, _) = setup();
    let impostor = Address::generate(&env);
    let reporter = Address::generate(&env);
    // mock_all_auths passes the signature check; the stored-admin check must
    // still reject an address that is not the admin.
    client.set_reporter(&impostor, &reporter, &true);
}

// ---- Record semantics -----------------------------------------------------

#[test]
fn two_reporters_write_to_one_record() {
    let (env, client, admin) = setup();
    let first = Address::generate(&env);
    let second = Address::generate(&env);
    let member = Address::generate(&env);
    client.set_reporter(&admin, &first, &true);
    client.set_reporter(&admin, &second, &true);

    client.report_completion(&first, &member, &CTX);
    client.report_completion(&second, &member, &CTX);
    client.report_default(&second, &member, &CTX);

    let r = client.get_reputation(&member);
    assert_eq!(r.completed, 2);
    assert_eq!(r.defaulted, 1);
}

#[test]
fn revoking_a_reporter_keeps_what_it_already_wrote() {
    let (env, client, admin) = setup();
    let reporter = Address::generate(&env);
    let member = Address::generate(&env);
    client.set_reporter(&admin, &reporter, &true);
    client.report_completion(&reporter, &member, &CTX);

    client.set_reporter(&admin, &reporter, &false);

    // Revocation is forward-only: it stops future writes without rewriting
    // history.
    let r = client.get_reputation(&member);
    assert_eq!(r.completed, 1);
    assert_eq!(r.defaulted, 0);
}

#[test]
fn records_are_kept_per_member() {
    let (env, client, admin) = setup();
    let reporter = Address::generate(&env);
    let good = Address::generate(&env);
    let bad = Address::generate(&env);
    client.set_reporter(&admin, &reporter, &true);

    client.report_completion(&reporter, &good, &CTX);
    client.report_default(&reporter, &bad, &CTX);

    let rg = client.get_reputation(&good);
    assert_eq!(rg.completed, 1);
    assert_eq!(rg.defaulted, 0);

    let rb = client.get_reputation(&bad);
    assert_eq!(rb.completed, 0);
    assert_eq!(rb.defaulted, 1);
}

// ---- Authorization --------------------------------------------------------

/// The single top-level authorisation `signer` gives for `fn_name(args)` on
/// the reputation contract.
fn expected_auth(
    client: &ReputationContractClient,
    signer: &Address,
    fn_name: &str,
    args: Vec<Val>,
) -> std::vec::Vec<(Address, AuthorizedInvocation)> {
    std::vec![(
        signer.clone(),
        AuthorizedInvocation {
            function: AuthorizedFunction::Contract((
                client.address.clone(),
                Symbol::new(&client.env, fn_name),
                args,
            )),
            sub_invocations: std::vec![],
        },
    )]
}

#[test]
fn set_reporter_requires_admin_auth() {
    let (env, client, admin) = setup();
    let reporter = Address::generate(&env);
    client.set_reporter(&admin, &reporter, &true);
    assert_eq!(
        env.auths(),
        expected_auth(
            &client,
            &admin,
            "set_reporter",
            (admin.clone(), reporter.clone(), true).into_val(&env),
        )
    );
}

#[test]
fn report_completion_requires_reporter_auth() {
    let (env, client, admin) = setup();
    let reporter = Address::generate(&env);
    let member = Address::generate(&env);
    client.set_reporter(&admin, &reporter, &true);
    client.report_completion(&reporter, &member, &CTX);
    assert_eq!(
        env.auths(),
        expected_auth(
            &client,
            &reporter,
            "report_completion",
            (reporter.clone(), member.clone(), CTX).into_val(&env),
        )
    );
}

#[test]
fn report_default_requires_reporter_auth() {
    let (env, client, admin) = setup();
    let reporter = Address::generate(&env);
    let member = Address::generate(&env);
    client.set_reporter(&admin, &reporter, &true);
    client.report_default(&reporter, &member, &CTX);
    assert_eq!(
        env.auths(),
        expected_auth(
            &client,
            &reporter,
            "report_default",
            (reporter.clone(), member.clone(), CTX).into_val(&env),
        )
    );
}

#[test]
fn set_reporter_without_admin_auth_fails() {
    let (env, client, admin) = setup();
    let reporter = Address::generate(&env);
    env.set_auths(&[]);
    assert!(client.try_set_reporter(&admin, &reporter, &true).is_err());
}

#[test]
fn report_completion_without_reporter_auth_fails() {
    let (env, client, admin) = setup();
    let reporter = Address::generate(&env);
    let member = Address::generate(&env);
    client.set_reporter(&admin, &reporter, &true);
    env.set_auths(&[]);
    assert!(client
        .try_report_completion(&reporter, &member, &CTX)
        .is_err());
    assert_eq!(client.get_reputation(&member).completed, 0);
}

#[test]
fn report_default_without_reporter_auth_fails() {
    let (env, client, admin) = setup();
    let reporter = Address::generate(&env);
    let member = Address::generate(&env);
    client.set_reporter(&admin, &reporter, &true);
    env.set_auths(&[]);
    assert!(client.try_report_default(&reporter, &member, &CTX).is_err());
    assert_eq!(client.get_reputation(&member).defaulted, 0);
}

#[test]
fn unlisted_reporter_with_auth_fails_not_reporter() {
    let (env, client, _) = setup();
    let rogue = Address::generate(&env);
    let member = Address::generate(&env);
    assert_eq!(
        client.try_report_default(&rogue, &member, &CTX),
        Err(Ok(Error::NotReporter.into()))
    );
}

// ---- Counter overflow (#106) ----------------------------------------------

/// Writes `rep` straight into the member's record via `RepV2`, bypassing the
/// reporter path so a test can start from a counter near `u32::MAX`.
fn seed(env: &Env, client: &ReputationContractClient, member: &Address, rep: &Reputation) {
    env.as_contract(&client.address, || {
        env.storage()
            .persistent()
            .set(&crate::types::DataKey::RepV2(member.clone()), rep);
    });
}

#[test]
fn completed_counter_overflow_panics_and_keeps_record() {
    let (env, client, admin) = setup();
    let reporter = Address::generate(&env);
    let member = Address::generate(&env);
    client.set_reporter(&admin, &reporter, &true);
    seed(&env, &client, &member, &rep(u32::MAX - 1, 7));

    client.report_completion(&reporter, &member, &CTX);
    assert_eq!(client.get_reputation(&member).completed, u32::MAX);

    assert!(client
        .try_report_completion(&reporter, &member, &CTX)
        .is_err());
    let r = client.get_reputation(&member);
    assert_eq!(r.completed, u32::MAX);
    assert_eq!(r.defaulted, 7);
}

#[test]
fn defaulted_counter_overflow_panics_and_keeps_record() {
    let (env, client, admin) = setup();
    let reporter = Address::generate(&env);
    let member = Address::generate(&env);
    client.set_reporter(&admin, &reporter, &true);
    seed(&env, &client, &member, &rep(3, u32::MAX));

    assert!(client.try_report_default(&reporter, &member, &CTX).is_err());
    let r = client.get_reputation(&member);
    assert_eq!(r.completed, 3);
    assert_eq!(r.defaulted, u32::MAX);

    client.report_completion(&reporter, &member, &CTX);
    assert_eq!(client.get_reputation(&member).completed, 4);
}

#[test]
#[should_panic(expected = "overflow")]
fn completed_counter_overflow_is_an_arithmetic_panic() {
    let (env, client, admin) = setup();
    let reporter = Address::generate(&env);
    let member = Address::generate(&env);
    client.set_reporter(&admin, &reporter, &true);
    seed(&env, &client, &member, &rep(u32::MAX, 0));
    client.report_completion(&reporter, &member, &CTX);
}

// ---- Events and long-lived history (#74, #76, #77) ------------------------

#[test]
fn initialize_emits_init_event_with_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(ReputationContract, ());
    let client = ReputationContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let topics: Vec<Val> = vec![
        &env,
        symbol_short!("rep").into_val(&env),
        symbol_short!("init").into_val(&env),
    ];
    let expected = vec![&env, (id.clone(), topics, admin.into_val(&env))];
    assert_eq!(env.events().all(), expected);
}

#[test]
fn outcome_events_carry_the_circle_context() {
    let (env, client, admin) = setup();
    let reporter = Address::generate(&env);
    let member = Address::generate(&env);
    client.set_reporter(&admin, &reporter, &true);

    client.report_completion(&reporter, &member, &42);
    let topics: Vec<Val> = vec![
        &env,
        symbol_short!("rep").into_val(&env),
        symbol_short!("complete").into_val(&env),
    ];
    let expected = vec![
        &env,
        (
            client.address.clone(),
            topics,
            (member.clone(), 42u64).into_val(&env),
        ),
    ];
    assert_eq!(env.events().all(), expected);

    client.report_default(&reporter, &member, &99);
    let topics: Vec<Val> = vec![
        &env,
        symbol_short!("rep").into_val(&env),
        symbol_short!("default").into_val(&env),
    ];
    let expected = vec![
        &env,
        (
            client.address.clone(),
            topics,
            (member.clone(), 99u64).into_val(&env),
        ),
    ];
    assert_eq!(env.events().all(), expected);
}

#[test]
fn reading_a_record_refreshes_its_long_ttl() {
    let (env, client, admin) = setup();
    let reporter = Address::generate(&env);
    let member = Address::generate(&env);
    client.set_reporter(&admin, &reporter, &true);
    client.report_completion(&reporter, &member, &1);

    let key = crate::types::DataKey::RepV2(member.clone());
    let initial_ttl =
        env.as_contract(&client.address, || env.storage().persistent().get_ttl(&key));
    assert!(
        initial_ttl >= ttl_helpers::LONG_TTL_EXTEND,
        "record should start with the long TTL, got {initial_ttl}"
    );
    let original_expiry = initial_ttl;

    env.ledger()
        .with_mut(|l| l.sequence_number = original_expiry - 1);
    let remaining =
        env.as_contract(&client.address, || env.storage().persistent().get_ttl(&key));
    assert_eq!(remaining, 1);
    assert_eq!(client.get_reputation(&member).completed, 1);

    let refreshed =
        env.as_contract(&client.address, || env.storage().persistent().get_ttl(&key));
    assert!(
        refreshed >= ttl_helpers::LONG_TTL_EXTEND,
        "read should refresh to the long window, got {refreshed}"
    );

    env.ledger()
        .with_mut(|l| l.sequence_number = original_expiry + 1);
    let r = client.get_reputation(&member);
    assert_eq!(r.completed, 1);
    assert_eq!(r.defaulted, 0);
}

// ---- Reporter allow-list storage (#75) ------------------------------------

#[test]
fn reporter_flags_live_in_persistent_storage() {
    let (env, client, admin) = setup();
    let reporter = Address::generate(&env);
    client.set_reporter(&admin, &reporter, &true);

    let key = crate::types::DataKey::Reporter(reporter);
    let persistent = env.as_contract(&client.address, || env.storage().persistent().has(&key));
    let instance = env.as_contract(&client.address, || env.storage().instance().has(&key));
    assert!(persistent, "reporter flag must be persistent");
    assert!(!instance, "reporter flag must not stay in instance storage");
}

// ---- Issue #78: timestamps ------------------------------------------------

#[test]
fn report_completion_records_timestamp() {
    let (env, client, admin) = setup();
    let reporter = Address::generate(&env);
    let member = Address::generate(&env);
    client.set_reporter(&admin, &reporter, &true);

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);
    client.report_completion(&reporter, &member, &CTX);

    let r = client.get_reputation(&member);
    assert_eq!(r.last_completed_at, Some(1_000_000));
    assert_eq!(r.last_defaulted_at, None);
}

#[test]
fn report_default_records_timestamp() {
    let (env, client, admin) = setup();
    let reporter = Address::generate(&env);
    let member = Address::generate(&env);
    client.set_reporter(&admin, &reporter, &true);

    env.ledger().with_mut(|l| l.timestamp = 2_000_000);
    client.report_default(&reporter, &member, &CTX);

    let r = client.get_reputation(&member);
    assert_eq!(r.last_defaulted_at, Some(2_000_000));
    assert_eq!(r.last_completed_at, None);
}

#[test]
fn latest_timestamp_overwrites_earlier_one() {
    let (env, client, admin) = setup();
    let reporter = Address::generate(&env);
    let member = Address::generate(&env);
    client.set_reporter(&admin, &reporter, &true);

    env.ledger().with_mut(|l| l.timestamp = 1_000);
    client.report_completion(&reporter, &member, &CTX);

    env.ledger().with_mut(|l| l.timestamp = 2_000);
    client.report_completion(&reporter, &member, &CTX);

    assert_eq!(client.get_reputation(&member).last_completed_at, Some(2_000));
}

#[test]
fn v1_record_migrates_on_first_read_with_no_timestamps() {
    let (env, client, _) = setup();
    let member = Address::generate(&env);

    // Write a legacy V1 record directly to storage.
    env.as_contract(&client.address, || {
        env.storage().persistent().set(
            &crate::types::DataKey::Rep(member.clone()),
            &crate::types::ReputationV1 {
                completed: 5,
                defaulted: 2,
            },
        );
    });

    let r = client.get_reputation(&member);
    assert_eq!(r.completed, 5);
    assert_eq!(r.defaulted, 2);
    assert_eq!(r.last_completed_at, None);
    assert_eq!(r.last_defaulted_at, None);

    // After migration the V2 key must exist.
    let has_v2 = env.as_contract(&client.address, || {
        env.storage()
            .persistent()
            .has(&crate::types::DataKey::RepV2(member.clone()))
    });
    assert!(has_v2, "RepV2 key should exist after migration");
}

// ---- Issue #79: admin correction ------------------------------------------

#[test]
fn correct_overwrites_counters() {
    let (env, client, admin) = setup();
    let reporter = Address::generate(&env);
    let member = Address::generate(&env);
    client.set_reporter(&admin, &reporter, &true);
    client.report_completion(&reporter, &member, &CTX);
    client.report_completion(&reporter, &member, &CTX);
    client.report_default(&reporter, &member, &CTX);

    let reason = Bytes::from_array(&env, &[0xdeu8; 32]);
    client.correct(&member, &5, &0, &reason);

    let r = client.get_reputation(&member);
    assert_eq!(r.completed, 5);
    assert_eq!(r.defaulted, 0);
}

#[test]
fn correct_preserves_timestamps() {
    let (env, client, admin) = setup();
    let reporter = Address::generate(&env);
    let member = Address::generate(&env);
    client.set_reporter(&admin, &reporter, &true);

    env.ledger().with_mut(|l| l.timestamp = 9_999);
    client.report_completion(&reporter, &member, &CTX);

    let reason = Bytes::from_array(&env, &[0x01u8; 32]);
    client.correct(&member, &10, &1, &reason);

    let r = client.get_reputation(&member);
    assert_eq!(r.completed, 10);
    assert_eq!(r.defaulted, 1);
    assert_eq!(r.last_completed_at, Some(9_999));
}

#[test]
fn correct_without_admin_auth_fails() {
    let (env, client, _) = setup();
    let member = Address::generate(&env);
    let reason = Bytes::from_array(&env, &[0x00u8; 32]);
    env.set_auths(&[]);
    assert!(client.try_correct(&member, &0, &0, &reason).is_err());
}

#[test]
fn correct_requires_admin_auth() {
    let (env, client, admin) = setup();
    let member = Address::generate(&env);
    let reason = Bytes::from_array(&env, &[0xabu8; 32]);
    client.correct(&member, &3, &1, &reason);
    assert_eq!(
        env.auths(),
        expected_auth(
            &client,
            &admin,
            "correct",
            (member.clone(), 3u32, 1u32, reason.clone()).into_val(&env),
        )
    );
}

// ---- Issue #80: score -----------------------------------------------------

#[test]
fn score_zero_for_unknown_member() {
    let (env, client, _) = setup();
    let stranger = Address::generate(&env);
    assert_eq!(client.score(&stranger), 0);
}

#[test]
fn score_perfect_with_no_defaults() {
    let (env, client, admin) = setup();
    let reporter = Address::generate(&env);
    let member = Address::generate(&env);
    client.set_reporter(&admin, &reporter, &true);
    client.report_completion(&reporter, &member, &CTX);
    client.report_completion(&reporter, &member, &CTX);
    assert_eq!(client.score(&member), 10_000);
}

#[test]
fn score_zero_with_no_completions() {
    let (env, client, admin) = setup();
    let reporter = Address::generate(&env);
    let member = Address::generate(&env);
    client.set_reporter(&admin, &reporter, &true);
    client.report_default(&reporter, &member, &CTX);
    assert_eq!(client.score(&member), 0);
}

#[test]
fn score_mixed_record() {
    let (env, client, admin) = setup();
    let reporter = Address::generate(&env);
    let member = Address::generate(&env);
    client.set_reporter(&admin, &reporter, &true);
    // 3 completions, 1 default → 3/4 × 10 000 = 7 500
    client.report_completion(&reporter, &member, &CTX);
    client.report_completion(&reporter, &member, &CTX);
    client.report_completion(&reporter, &member, &CTX);
    client.report_default(&reporter, &member, &CTX);
    assert_eq!(client.score(&member), 7_500);
}

// ---- Issue #81: contract address check ------------------------------------

#[test]
fn account_address_rejected_as_reporter() {
    let (env, client, admin) = setup();
    // register_stellar_asset_contract_v2 creates a real Stellar account entry
    // in the test ledger, so executable() returns Some(Executable::Account).
    let sac = env.register_stellar_asset_contract_v2(Address::generate(&env));
    let account_addr = sac.issuer().address();
    assert_eq!(
        client.try_set_reporter(&admin, &account_addr, &true),
        Err(Ok(Error::NotContractAddress.into()))
    );
}

#[test]
fn unregistered_contract_address_accepted_as_reporter() {
    // Address::generate creates an unregistered contract address.
    // executable() returns None for it, which is not Some(Account), so it
    // passes the check — an admin may whitelist a contract before it deploys.
    let (env, client, admin) = setup();
    let reporter = Address::generate(&env);
    assert!(client.try_set_reporter(&admin, &reporter, &true).is_ok());
}
