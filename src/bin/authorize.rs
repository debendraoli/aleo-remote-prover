use clap::{ArgGroup, Parser};
use remote_prover::{network_api_base, CurrentAleo, CurrentNetwork};
use reqwest::blocking::Client;
use reqwest::Url;
use snarkvm::prelude::{Address, Identifier, PrivateKey, Program, ProgramID, ViewKey};
use snarkvm::synthesizer::Process;
use std::{collections::HashSet, fs, io, path::PathBuf, str::FromStr, time::Duration};

struct RemoteFetcher {
    client: Client,
    base_url: Url,
}

impl RemoteFetcher {
    fn new(base_url: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let url = Url::parse(base_url)
            .map_err(|e| with_context(format!("invalid API base '{base_url}'"), e))?;

        let client = Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| with_context("failed to build HTTP client", e))?;

        Ok(Self {
            client,
            base_url: url,
        })
    }

    fn fetch_latest_edition(
        &self,
        program_id: &str,
    ) -> Result<Option<u16>, Box<dyn std::error::Error>> {
        let mut url = self.base_url.clone();
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| boxed_err("API base URL must be absolute"))?;
            segments.pop_if_empty();
            segments.push("program");
            segments.push(program_id);
            segments.push("latest_edition");
        }

        let response = self
            .client
            .get(url.clone())
            .header("Accept", "application/json")
            .send()
            .map_err(|e| with_context(format!("failed to fetch latest edition for '{program_id}'"), e))?;

        if !response.status().is_success() {
            // If 404, the program may not have editions yet
            if response.status().as_u16() == 404 {
                return Ok(None);
            }
            return Err(boxed_err(format!(
                "latest edition request for '{}' failed with status {}",
                program_id,
                response.status()
            )));
        }

        let body = response
            .text()
            .map_err(|e| with_context("failed to read latest edition response", e))?;

        let edition: u16 = body
            .trim()
            .parse()
            .map_err(|e| with_context(format!("failed to parse edition number for '{program_id}'"), e))?;

        Ok(Some(edition))
    }

    fn fetch_program(
        &self,
        program_id: &str,
        edition: Option<u16>,
    ) -> Result<Program<CurrentNetwork>, Box<dyn std::error::Error>> {
        let mut url = self.base_url.clone();
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| boxed_err("API base URL must be absolute"))?;
            segments.pop_if_empty();
            segments.push("program");
            segments.push(program_id);
            if let Some(edition) = edition {
                segments.push(&edition.to_string());
            }
        }

        eprintln!("ℹ️  Fetching program '{}' from {}", program_id, url);

        let response = self
            .client
            .get(url.clone())
            .header("Accept", "application/json")
            .send()
            .map_err(|e| with_context(format!("failed to fetch program '{program_id}'"), e))?;

        if !response.status().is_success() {
            return Err(boxed_err(format!(
                "request to {} failed with status {}",
                url,
                response.status()
            )));
        }

        let body = response
            .text()
            .map_err(|e| with_context("failed to read response body", e))?;
        let trimmed = body.trim();
        let source = if trimmed.starts_with('"') {
            serde_json::from_str::<String>(trimmed)
                .map_err(|e| with_context("failed to decode program body", e))?
        } else {
            body
        };

        Program::<CurrentNetwork>::from_str(&source)
            .map_err(|e| with_context(format!("failed to parse program '{program_id}'"), e))
    }
}

/// Generate an Aleo authorization string for a program execution.
#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Generate an Aleo authorization string for the remote prover",
    long_about = None,
)]
#[command(group(
    ArgGroup::new("program_source")
        .required(true)
        .args(["program_file", "program_id"]),
))]
struct Args {
    /// Path to the compiled Aleo program file (e.g. build/main.aleo)
    #[arg(short = 'f', long = "program-file", value_name = "FILE")]
    program_file: Option<PathBuf>,

    /// Program ID of an on-chain deployment (e.g. my_app.aleo)
    #[arg(short = 'p', long = "program-id", value_name = "ID")]
    program_id: Option<String>,

    /// Specific program edition to fetch (only valid with --program-id)
    #[arg(long, value_name = "EDITION")]
    edition: Option<u16>,

    /// Override the Provable API base URL (default: uses compile-time network)
    #[arg(long, value_name = "URL")]
    api_base: Option<String>,

    /// Function name to execute within the program
    #[arg(short = 'F', long, value_name = "FUNCTION")]
    function: String,

    /// Repeated inputs passed to the function (use Leo literal syntax)
    #[arg(short = 'i', long = "input", value_name = "VALUE")]
    inputs: Vec<String>,

    /// Private key that will authorize the execution
    #[arg(short = 'k', long = "private-key", value_name = "KEY")]
    private_key: String,

    /// Print the derived account address to stderr for verification
    #[arg(long, default_value_t = false)]
    print_account: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let args = Args::parse();

    if args.program_file.is_some() && args.edition.is_some() {
        return Err(boxed_err(
            "--edition is only supported alongside --program-id",
        ));
    }

    let (program, remote_fetcher) = load_program(&args)?;

    let function_id = Identifier::<CurrentNetwork>::from_str(&args.function)
        .map_err(|e| with_context(format!("failed to parse function '{}'", args.function), e))?;

    let mut process = Process::<CurrentNetwork>::load()
        .map_err(|e| with_context("failed to initialize proving process", e))?;

    if let Some(fetcher) = remote_fetcher.as_ref() {
        let mut visited = HashSet::new();
        visited.insert(program.id().to_string());
        load_remote_dependencies(&mut process, fetcher, &program, &mut visited)?;
    }

    process
        .add_program(&program)
        .map_err(|e| with_context("failed to add program to process", e))?;

    let private_key = PrivateKey::<CurrentNetwork>::from_str(&args.private_key)
        .map_err(|e| with_context("failed to parse private key", e))?;

    let mut rng = rand::thread_rng();
    let authorization = process
        .authorize::<CurrentAleo, _>(
            &private_key,
            program.id(),
            function_id,
            args.inputs.iter().map(String::as_str),
            &mut rng,
        )
        .map_err(|e| with_context("failed to authorize execution", e))?;

    println!("{authorization}");

    if args.print_account {
        let view_key = ViewKey::<CurrentNetwork>::try_from(&private_key)
            .map_err(|e| with_context("failed to derive view key", e))?;
        let address = Address::<CurrentNetwork>::try_from(&view_key)
            .map_err(|e| with_context("failed to derive address", e))?;
        eprintln!("Account address: {address}");
    }

    Ok(())
}

fn load_program(
    args: &Args,
) -> Result<(Program<CurrentNetwork>, Option<RemoteFetcher>), Box<dyn std::error::Error>> {
    if let Some(path) = &args.program_file {
        return Ok((load_local_program(path)?, None));
    }

    let program_id = args
        .program_id
        .as_ref()
        .expect("clap ensures a program source is provided")
        .trim();

    if program_id.is_empty() {
        return Err(boxed_err("--program-id must not be empty"));
    }

    ProgramID::<CurrentNetwork>::from_str(program_id)
        .map_err(|e| with_context(format!("failed to parse program ID '{program_id}'"), e))?;

    let base_url = args
        .api_base
        .as_deref()
        .unwrap_or_else(|| network_api_base());

    let fetcher = RemoteFetcher::new(base_url)?;

    // Use explicit edition if provided, otherwise fetch the latest edition
    let edition = match args.edition {
        Some(e) => Some(e),
        None => fetcher.fetch_latest_edition(program_id)?,
    };

    let program = fetcher.fetch_program(program_id, edition)?;
    Ok((program, Some(fetcher)))
}

fn load_local_program(path: &PathBuf) -> Result<Program<CurrentNetwork>, Box<dyn std::error::Error>> {
    let source = fs::read_to_string(path)
        .map_err(|e| with_context(format!("failed to read program {}", path.display()), e))?;

    Program::<CurrentNetwork>::from_str(&source)
        .map_err(|e| with_context("failed to parse program", e))
}

fn load_remote_dependencies(
    process: &mut Process<CurrentNetwork>,
    fetcher: &RemoteFetcher,
    parent: &Program<CurrentNetwork>,
    visited: &mut HashSet<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    for (import_id, _) in parent.imports() {
        let import_str = import_id.to_string();
        if !visited.insert(import_str.clone()) {
            continue;
        }

        // Fetch the latest edition for the dependency
        let edition = fetcher.fetch_latest_edition(&import_str)?;
        let dependency = fetcher.fetch_program(&import_str, edition)?;
        load_remote_dependencies(process, fetcher, &dependency, visited)?;
        process.add_program(&dependency).map_err(|e| {
            with_context(
                format!("failed to add dependency '{import_str}' to process"),
                e,
            )
        })?;
    }

    Ok(())
}

fn boxed_err(message: impl Into<String>) -> Box<dyn std::error::Error> {
    Box::new(io::Error::other(message.into()))
}

fn with_context<E: std::fmt::Display>(
    message: impl Into<String>,
    error: E,
) -> Box<dyn std::error::Error> {
    boxed_err(format!("{}: {error}", message.into()))
}
