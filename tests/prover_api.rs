use parking_lot::RwLock;
use std::{str::FromStr, sync::Arc};

use remote_prover::{
    prover_routes, AuthorizationPayload, CurrentAleo, CurrentNetwork, ProveRequest, ProverConfig,
};
use serde_json::Value;
use snarkvm::{
    prelude::{Identifier, PrivateKey, Program},
    synthesizer::Process,
};
use warp::http::StatusCode;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn healthcheck_root_returns_ok() {
    let process = Arc::new(RwLock::new(
        Process::<CurrentNetwork>::load().expect("failed to load process"),
    ));
    let config = Arc::new(ProverConfig::default());
    let routes = prover_routes(process, config);

    let response = warp::test::request()
        .method("GET")
        .path("/")
        .reply(&routes)
        .await;

    assert_eq!(response.status(), StatusCode::OK, "unexpected status");

    let json: Value = serde_json::from_slice(response.body()).expect("invalid JSON body");
    assert_eq!(json["status"], "ok");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn prove_simple_contract_execution() {
    // Build a program that performs a simple addition with public inputs.
    const PROGRAM_SOURCE: &str = r#"
program contract_execution.aleo;

function add_public:
    input r0 as u32.public;
    input r1 as u32.public;
    add r0 r1 into r2;
    output r2 as u32.public;
"#;

    let program = Program::<CurrentNetwork>::from_str(PROGRAM_SOURCE)
        .expect("failed to parse sample program");

    // Prepare the proving process and register the program.
    let mut process_instance = Process::<CurrentNetwork>::load().expect("failed to load process");
    process_instance
        .add_program(&program)
        .expect("failed to add sample program");

    // Produce an authorization for the program execution.
    let function_name =
        Identifier::<CurrentNetwork>::from_str("add_public").expect("missing function name");
    let mut rng = rand::thread_rng();
    let private_key =
        PrivateKey::<CurrentNetwork>::new(&mut rng).expect("failed to create private key");
    let authorization = process_instance
        .authorize::<CurrentAleo, _>(
            &private_key,
            program.id(),
            function_name,
            ["5u32", "7u32"].into_iter(),
            &mut rng,
        )
        .expect("failed to authorize execution");

    let process = Arc::new(RwLock::new(process_instance));

    let authorization_value = serde_json::from_str(&authorization.to_string())
        .expect("authorization should be valid JSON");

    let request_body = ProveRequest {
        authorization: AuthorizationPayload::Json(authorization_value),
        broadcast: Some(false),
        network: None,
    };

    let config = Arc::new(ProverConfig::default());
    let routes = prover_routes(process, config);

    let response = warp::test::request()
        .method("POST")
        .path("/prove")
        .json(&request_body)
        .reply(&routes)
        .await;

    assert_eq!(response.status(), StatusCode::OK, "unexpected status");

    let json: Value = serde_json::from_slice(response.body()).expect("invalid JSON body");
    assert_eq!(json["status"], "success");
    assert_eq!(json["summary"]["transitions"], 1);
}
