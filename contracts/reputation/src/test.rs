#![cfg(test)]

extern crate std;

use crate::{errors::Error, types::Reputation, ReputationContract, ReputationContractClient};
use soroban_sdk::{
    symbol_short,
    testutils::{
        storage::Persistent as _, Address as _, AuthorizedFunction, AuthorizedInvocation, Events,
        Ledger,
    },
    vec, Address, Env, IntoVal, Symbol, Val, Vec,
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

#[test]
fn reports_increment_counts() {
    let (env, client, admin) = setup();
    let reporter = Address::generate(&env);
    let member = Address::generate(&env);
    client.set_reporter(&admin, &reporter, &true);

    client.report_completion(&reporter, &member, &CTX);
    client.report_completion(&reporter, &member, &CTX);
    client.report_default(&reporter, &member, &CTX);

    assert_eq!(
        client.get_reputation(&member),
        Reputation {
            completed: 2,
            defaulted: 1
        }
    );
}

#[test]
fn unknown_member_reads_zero() {
    let (env, client, _) = setup();
    let stranger = Address::generate(&env);
    assert_eq!(
        client.get_reputation(&stranger),
        Reputation {
            completed: 0,
            defaulted: 0
        }
    );
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

    // A member's history spans every circle contract allowed to report, not
    // just the one that created the circle.
    client.report_completion(&first, &member, &CTX);
    client.report_completion(&second, &member, &CTX);
    client.report_default(&second, &member, &CTX);

    assert_eq!(
        client.get_reputation(&member),
        Reputation {
            completed: 2,
            defaulted: 1
        }
    );
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
    // history. An admin must not be able to launder a default away by
    // revoking the reporter that recorded it.
    assert_eq!(
        client.get_reputation(&member),
        Reputation {
            completed: 1,
            defaulted: 0
        }
    );
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

    assert_eq!(
        client.get_reputation(&good),
        Reputation {
            completed: 1,
            defaulted: 0
        }
    );
    assert_eq!(
        client.get_reputation(&bad),
        Reputation {
            completed: 0,
            defaulted: 1
        }
    );
}

// ---- Authorization --------------------------------------------------------
//
// `setup()` uses `mock_all_auths`, so a dropped `require_auth` would not make
// any other test fail. These assert the exact address whose authorisation
// each call consumes, and that the call fails when that authorisation is
// missing.

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
    // The reporter authorises; the member being reported does not.
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
    // Allow-listed, but the reporter did not sign: rejected before any write.
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
    // Auth alone is not enough: the allow-list check must also pass.
    assert_eq!(
        client.try_report_default(&rogue, &member, &CTX),
        Err(Ok(Error::NotReporter.into()))
    );
}

// ---- Counter overflow (#106) ----------------------------------------------

/// Writes `rep` straight into the member's record, bypassing the reporter
/// path, so a test can start from a counter near `u32::MAX`.
fn seed(env: &Env, client: &ReputationContractClient, member: &Address, rep: &Reputation) {
    env.as_contract(&client.address, || {
        env.storage()
            .persistent()
            .set(&crate::types::DataKey::Rep(member.clone()), rep);
    });
}

#[test]
fn completed_counter_overflow_panics_and_keeps_record() {
    let (env, client, admin) = setup();
    let reporter = Address::generate(&env);
    let member = Address::generate(&env);
    client.set_reporter(&admin, &reporter, &true);
    seed(
        &env,
        &client,
        &member,
        &Reputation {
            completed: u32::MAX - 1,
            defaulted: 7,
        },
    );

    // The last representable value is still reachable.
    client.report_completion(&reporter, &member, &CTX);
    assert_eq!(client.get_reputation(&member).completed, u32::MAX);

    // `completed += 1` on u32::MAX trips the overflow check (enabled in both
    // the test profile and the release wasm profile): the call aborts rather
    // than wrapping to 0, and the stored record is left untouched.
    assert!(client
        .try_report_completion(&reporter, &member, &CTX)
        .is_err());
    assert_eq!(
        client.get_reputation(&member),
        Reputation {
            completed: u32::MAX,
            defaulted: 7
        }
    );
}

#[test]
fn defaulted_counter_overflow_panics_and_keeps_record() {
    let (env, client, admin) = setup();
    let reporter = Address::generate(&env);
    let member = Address::generate(&env);
    client.set_reporter(&admin, &reporter, &true);
    seed(
        &env,
        &client,
        &member,
        &Reputation {
            completed: 3,
            defaulted: u32::MAX,
        },
    );

    assert!(client.try_report_default(&reporter, &member, &CTX).is_err());
    assert_eq!(
        client.get_reputation(&member),
        Reputation {
            completed: 3,
            defaulted: u32::MAX
        }
    );

    // A saturated `defaulted` counter does not block the other counter.
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
    seed(
        &env,
        &client,
        &member,
        &Reputation {
            completed: u32::MAX,
            defaulted: 0,
        },
    );
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

    let key = crate::types::DataKey::Rep(member.clone());
    let initial_ttl = env.as_contract(&client.address, || env.storage().persistent().get_ttl(&key));
    assert!(
        initial_ttl >= ttl_helpers::LONG_TTL_EXTEND,
        "record should start with the long TTL, got {initial_ttl}"
    );
    let original_expiry = initial_ttl;

    // One ledger before the record would expire, a read refreshes it.
    env.ledger()
        .with_mut(|l| l.sequence_number = original_expiry - 1);
    let remaining = env.as_contract(&client.address, || env.storage().persistent().get_ttl(&key));
    assert_eq!(remaining, 1);
    assert_eq!(client.get_reputation(&member).completed, 1);

    let refreshed = env.as_contract(&client.address, || env.storage().persistent().get_ttl(&key));
    assert!(
        refreshed >= ttl_helpers::LONG_TTL_EXTEND,
        "read should refresh to the long window, got {refreshed}"
    );

    // Past the original expiry the record still reads back intact.
    env.ledger()
        .with_mut(|l| l.sequence_number = original_expiry + 1);
    assert_eq!(
        client.get_reputation(&member),
        Reputation {
            completed: 1,
            defaulted: 0
        }
    );
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
