use crate::{CurrentAleo, CurrentNetwork};
use parking_lot::RwLock;
use snarkvm::algorithms::snark::varuna::VarunaVersion;
use snarkvm::ledger::query::QueryTrait;
use snarkvm::ledger::{query::Query, store::helpers::memory::BlockMemory};
use snarkvm::prelude::*;
use snarkvm::synthesizer::Process;
use std::sync::Arc;

#[derive(Clone, serde::Serialize)]
pub(crate) struct FeeInfo {
    pub(crate) kind: &'static str,
    pub(crate) transition_id: String,
    pub(crate) amount_microcredits: String,
    pub(crate) base_microcredits: String,
    pub(crate) priority_microcredits: String,
    pub(crate) payer: Option<String>,
    pub(crate) global_state_root: String,
    pub(crate) num_finalize_operations: usize,
}

pub(crate) struct ProvingArtifacts {
    pub(crate) summary: serde_json::Value,
    pub(crate) transaction: Transaction<CurrentNetwork>,
    pub(crate) execution_id: String,
    pub(crate) fee_info: Option<FeeInfo>,
}

pub fn prove_transaction(
    process: Arc<RwLock<Process<CurrentNetwork>>>,
    authorization: Authorization<CurrentNetwork>,
    fee_authorization: Option<Authorization<CurrentNetwork>>,
    rest_endpoint: String,
) -> Result<ProvingArtifacts, String> {
    let mut rng = rand::thread_rng();
    let query =
        Query::<CurrentNetwork, BlockMemory<CurrentNetwork>>::try_from(rest_endpoint.as_str())
            .map_err(|err| format!("Failed to initialize query: {err}"))?;

    let consensus_version = {
        let height = query
            .current_block_height()
            .map_err(|err| format!("Failed to fetch current block height: {err}"))?;
        <CurrentNetwork as Network>::CONSENSUS_VERSION(height)
            .map_err(|err| format!("Failed to determine consensus version: {err}"))?
    };

    let varuna_version =
        if (ConsensusVersion::V1..=ConsensusVersion::V3).contains(&consensus_version) {
            VarunaVersion::V1
        } else {
            VarunaVersion::V2
        };

    let locator = {
        let request = authorization
            .peek_next()
            .map_err(|err| format!("Failed to inspect authorization: {err}"))?;
        Locator::new(*request.program_id(), *request.function_name()).to_string()
    };

    let (response, mut trace) = {
        let guard = process.read();
        authorization
            .check_valid_edition(&guard, consensus_version)
            .map_err(|err| err.to_string())?;
        authorization
            .check_valid_records(consensus_version)
            .map_err(|err| err.to_string())?;
        guard
            .execute::<CurrentAleo, _>(authorization.clone(), &mut rng)
            .map_err(|err| err.to_string())?
    };

    trace.prepare(&query).map_err(|err| err.to_string())?;
    let execution = trace
        .prove_execution::<CurrentAleo, _>(&locator, varuna_version, &mut rng)
        .map_err(|err| err.to_string())?;

    let execution_id = execution
        .to_execution_id()
        .map_err(|err| err.to_string())?
        .to_string();

    let call_metrics_json = trace
        .call_metrics()
        .iter()
        .map(|metrics| {
            serde_json::json!({
                "program_id": metrics.program_id.to_string(),
                "function": metrics.function_name.to_string(),
                "instructions": metrics.num_instructions,
                "request_constraints": metrics.num_request_constraints,
                "function_constraints": metrics.num_function_constraints,
                "response_constraints": metrics.num_response_constraints,
            })
        })
        .collect::<Vec<_>>();

    let summary = serde_json::json!({
        "locator": locator,
        "output_ids": response
            .output_ids()
            .iter()
            .map(|output_id| format!("{output_id:?}"))
            .collect::<Vec<_>>(),
        "outputs": response
            .outputs()
            .iter()
            .map(|output| format!("{output:?}"))
            .collect::<Vec<_>>(),
        "transitions": trace.transitions().len(),
        "call_metrics": call_metrics_json,
        "is_fee": trace.is_fee(),
    });

    let (fee_for_transaction, fee_info) = if let Some(fee_auth) = fee_authorization {
        let mut fee_trace = {
            let guard = process.read();
            fee_auth
                .check_valid_edition(&guard, consensus_version)
                .map_err(|err| err.to_string())?;
            fee_auth
                .check_valid_records(consensus_version)
                .map_err(|err| err.to_string())?;
            let (_, trace) = guard
                .execute::<CurrentAleo, _>(fee_auth, &mut rng)
                .map_err(|err| err.to_string())?;
            trace
        };

        fee_trace.prepare(&query).map_err(|err| err.to_string())?;
        let fee = fee_trace
            .prove_fee::<CurrentAleo, _>(varuna_version, &mut rng)
            .map_err(|err| err.to_string())?;
        let fee_info = build_fee_info(&fee)?;
        (Some(fee), Some(fee_info))
    } else {
        (None, None)
    };

    let transaction = Transaction::from_execution(execution, fee_for_transaction)
        .map_err(|err| err.to_string())?;

    Ok(ProvingArtifacts {
        summary,
        transaction,
        execution_id,
        fee_info,
    })
}

fn build_fee_info(fee: &Fee<CurrentNetwork>) -> Result<FeeInfo, String> {
    let kind = if fee.is_fee_private() {
        "private"
    } else if fee.is_fee_public() {
        "public"
    } else {
        return Err("Fee transition is neither private nor public".to_string());
    };

    let amount_microcredits = fee.amount().map_err(|err| err.to_string())?.to_string();
    let base_microcredits = fee
        .base_amount()
        .map_err(|err| err.to_string())?
        .to_string();
    let priority_microcredits = fee
        .priority_amount()
        .map_err(|err| err.to_string())?
        .to_string();
    let payer = fee.payer().map(|addr| addr.to_string());

    Ok(FeeInfo {
        kind,
        transition_id: format!("{:?}", fee.transition_id()),
        amount_microcredits,
        base_microcredits,
        priority_microcredits,
        payer,
        global_state_root: format!("{:?}", fee.global_state_root()),
        num_finalize_operations: fee.num_finalize_operations(),
    })
}
