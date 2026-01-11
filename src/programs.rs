use crate::CurrentNetwork;
use parking_lot::RwLock;
use reqwest::Url;
use snarkvm::prelude::*;
use snarkvm::synthesizer::Process;
use std::{collections::HashSet, str::FromStr, sync::Arc};
use tracing::info;

struct ProgramWithEdition {
    program: Program<CurrentNetwork>,
    edition: u16,
}

pub async fn fetch_latest_edition(
    client: &reqwest::Client,
    base: &Url,
    program_id: &ProgramID<CurrentNetwork>,
) -> Result<Option<u16>, String> {
    let url = build_latest_edition_url(base, program_id)?;

    let response = client
        .get(url.clone())
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|err| format!("Failed to fetch latest edition for '{program_id}': {err}"))?;

    if !response.status().is_success() {
        if response.status().as_u16() == 404 {
            return Ok(None);
        }
        return Err(format!(
            "Latest edition request for '{program_id}' failed with status {}",
            response.status()
        ));
    }

    let body = response
        .text()
        .await
        .map_err(|err| format!("Failed to read latest edition for '{program_id}': {err}"))?;

    let edition: u16 = body
        .trim()
        .parse()
        .map_err(|err| format!("Failed to parse edition number for '{program_id}': {err}"))?;

    Ok(Some(edition))
}

fn build_latest_edition_url(
    base: &Url,
    program_id: &ProgramID<CurrentNetwork>,
) -> Result<Url, String> {
    let mut url = base.clone();
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| format!("Program API base '{}' must be absolute", base))?;
        segments.pop_if_empty();
        segments.push("program");
        segments.push(&program_id.to_string());
        segments.push("latest_edition");
    }
    Ok(url)
}

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
    let mut pending: Vec<ProgramWithEdition> = Vec::new();

    while let Some((program_id, ready)) = stack.pop() {
        if program_id == credits_program_id {
            continue;
        }

        {
            let guard = process.read();
            if guard.contains_program(&program_id) {
                continue;
            }
        }

        if ready {
            if let Some(idx) = pending.iter().position(|p| *p.program.id() == program_id) {
                let ProgramWithEdition { program, edition } = pending.swap_remove(idx);
                let mut guard = process.write();
                if !guard.contains_program(program.id()) {
                    guard
                        .add_program_with_edition(&program, edition)
                        .map_err(|err| {
                            format!(
                                "Failed to add program '{program_id}' (edition {edition}): {err}"
                            )
                        })?;
                }
            }
            continue;
        }

        if !scheduled.insert(program_id) {
            continue;
        }

        let (program, edition) =
            fetch_remote_program_with_edition(client, &base, &program_id).await?;
        let imports: Vec<_> = program.imports().keys().copied().collect();

        pending.push(ProgramWithEdition { program, edition });
        stack.push((program_id, true));
        for import_id in imports {
            stack.push((import_id, false));
        }
    }

    Ok(())
}

pub async fn fetch_remote_program_with_edition(
    client: &reqwest::Client,
    base: &Url,
    program_id: &ProgramID<CurrentNetwork>,
) -> Result<(Program<CurrentNetwork>, u16), String> {
    let edition = fetch_latest_edition(client, base, program_id)
        .await?
        .unwrap_or(0);

    let url = build_program_url(base, program_id, Some(edition))?;
    info!(
        "Fetching program '{}' (edition {}) from {}",
        program_id,
        edition,
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

    let program = Program::<CurrentNetwork>::from_str(&source)
        .map_err(|err| format!("Failed to parse program '{program_id}': {err}"))?;

    Ok((program, edition))
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
