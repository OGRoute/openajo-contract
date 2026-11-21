#![cfg(test)]

extern crate std;

use crate::{errors::Error, types::Reputation, ReputationContract, ReputationContractClient};
use soroban_sdk::{
    testutils::{Address as _, AuthorizedFunction, AuthorizedInvocation},
    Address, Env, IntoVal, Symbol, Val, Vec,
};

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

    client.report_completion(&reporter, &member);
    client.report_completion(&reporter, &member);
    client.report_default(&reporter, &member);

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
    client.report_completion(&rogue, &member);
}

#[test]
#[should_panic]
fn revoked_reporter_panics() {
    let (env, client, admin) = setup();
    let reporter = Address::generate(&env);
    let member = Address::generate(&env);
    client.set_reporter(&admin, &reporter, &true);
    client.set_reporter(&admin, &reporter, &false);
    client.report_default(&reporter, &member);
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
    client.report_completion(&first, &member);
    client.report_completion(&second, &member);
    client.report_default(&second, &member);

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
    client.report_completion(&reporter, &member);

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

    client.report_completion(&reporter, &good);
    client.report_default(&reporter, &bad);

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
    client.report_completion(&reporter, &member);
    // The reporter authorises; the member being reported does not.
    assert_eq!(
        env.auths(),
        expected_auth(
            &client,
            &reporter,
            "report_completion",
            (reporter.clone(), member.clone()).into_val(&env),
        )
    );
}

#[test]
fn report_default_requires_reporter_auth() {
    let (env, client, admin) = setup();
    let reporter = Address::generate(&env);
    let member = Address::generate(&env);
    client.set_reporter(&admin, &reporter, &true);
    client.report_default(&reporter, &member);
    assert_eq!(
        env.auths(),
        expected_auth(
            &client,
            &reporter,
            "report_default",
            (reporter.clone(), member.clone()).into_val(&env),
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
    assert!(client.try_report_completion(&reporter, &member).is_err());
    assert_eq!(client.get_reputation(&member).completed, 0);
}

#[test]
fn report_default_without_reporter_auth_fails() {
    let (env, client, admin) = setup();
    let reporter = Address::generate(&env);
    let member = Address::generate(&env);
    client.set_reporter(&admin, &reporter, &true);
    env.set_auths(&[]);
    assert!(client.try_report_default(&reporter, &member).is_err());
    assert_eq!(client.get_reputation(&member).defaulted, 0);
}

#[test]
fn unlisted_reporter_with_auth_fails_not_reporter() {
    let (env, client, _) = setup();
    let rogue = Address::generate(&env);
    let member = Address::generate(&env);
    // Auth alone is not enough: the allow-list check must also pass.
    assert_eq!(
        client.try_report_default(&rogue, &member),
        Err(Ok(Error::NotReporter.into()))
    );
}
