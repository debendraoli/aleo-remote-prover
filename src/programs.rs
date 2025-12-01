use crate::{config::Network, CurrentNetwork};
use parking_lot::RwLock;
use reqwest::Url;
use snarkvm::prelude::*;
use snarkvm::synthesizer::Process;
use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
    sync::Arc,
};

pub async fn ensure_programs_available(
    process: &Arc<RwLock<Process<CurrentNetwork>>>,
    client: &reqwest::Client,
    base_url: &str,
    authorization: &Authorization<CurrentNetwork>,
) -> Result<(), String> {
    let base = Url::parse(base_url)
        .map_err(|err| format!("Invalid program API base '{base_url}': {err}"))?;

    let mut stack: Vec<(ProgramID<CurrentNetwork>, bool)> = authorization
        .to_vec_deque()
        .into_iter()
        .map(|request| (*request.program_id(), false))
        .collect();
    stack.extend(
        authorization
            .transitions()
            .values()
            .map(|transition| (*transition.program_id(), false)),
    );

    let credits_program_id = ProgramID::<CurrentNetwork>::from_str("credits.aleo")
        .map_err(|err| format!("Failed to parse reference program ID: {err}"))?;

    let mut scheduled = HashSet::new();
    let mut pending: HashMap<ProgramID<CurrentNetwork>, Program<CurrentNetwork>> = HashMap::new();

    while let Some((program_id, ready)) = stack.pop() {
        if program_id == credits_program_id {
            continue;
        }

        {
            let guard = process.read();
            if guard.contains_program(&program_id) {
                if ready {
                    pending.remove(&program_id);
                }
                continue;
            }
        }

        if ready {
            if let Some(program) = pending.remove(&program_id) {
                let mut guard = process.write();
                if !guard.contains_program(program.id()) {
                    guard
                        .add_program(&program)
                        .map_err(|err| format!("Failed to add program '{program_id}': {err}"))?;
                }
            }
            continue;
        }

        if !scheduled.insert(program_id) {
            continue;
        }

        let program = fetch_remote_program(client, &base, &program_id).await?;
        let imports: Vec<_> = program.imports().keys().copied().collect();

        pending.insert(program_id, program);
        stack.push((program_id, true));
        for import_id in imports {
            stack.push((import_id, false));
        }
    }

    Ok(())
}

pub async fn fetch_remote_program(
    client: &reqwest::Client,
    base: &Url,
    program_id: &ProgramID<CurrentNetwork>,
) -> Result<Program<CurrentNetwork>, String> {
    let url = build_program_url(base, program_id, None)?;

    eprintln!(
        "ℹ️  Fetching missing program '{}' from {}",
        program_id,
        url.as_str()
    );

    let response = client
        .get(url.clone())
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|err| format!("Failed to fetch program '{program_id}': {err}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "Program '{program_id}' request failed with status {}",
            response.status()
        ));
    }

    let body = response
        .text()
        .await
        .map_err(|err| format!("Failed to read program '{program_id}': {err}"))?;
    let trimmed = body.trim();
    let source = if trimmed.starts_with('"') {
        serde_json::from_str::<String>(trimmed)
            .map_err(|err| format!("Failed to decode program '{program_id}': {err}"))?
    } else {
        body
    };

    Program::<CurrentNetwork>::from_str(&source)
        .map_err(|err| format!("Failed to parse program '{program_id}': {err}"))
}

fn build_program_url(
    base: &Url,
    program_id: &ProgramID<CurrentNetwork>,
    edition: Option<u16>,
) -> Result<Url, String> {
    let mut url = base.clone();
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| format!("Program API base '{}' must be absolute", base))?;
        segments.pop_if_empty();
        segments.push("program");
        segments.push(&program_id.to_string());
        if let Some(edition) = edition {
            segments.push(&edition.to_string());
        }
    }

    Ok(url)
}

#[allow(dead_code)]
pub fn build_network_program_url(
    network: Network,
    program_id: &ProgramID<CurrentNetwork>,
    edition: Option<u16>,
) -> Result<Url, String> {
    let base = Url::parse(&network.rest_base_url())
        .map_err(|err| format!("Invalid REST base for network: {err}"))?;
    build_program_url(&base, program_id, edition)
}
