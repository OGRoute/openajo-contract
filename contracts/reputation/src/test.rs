#![cfg(test)]

use crate::{types::Reputation, ReputationContract, ReputationContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

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
