//! Benchmark harness for testing rust-crate-sources-proxy research quality.
//!
//! Runs a research prompt through the proxy + Claude Code, then validates
//! the response against expected results using another Claude Code instance.

use anyhow::Result;
use clap::Parser;
use sacp_tokio::AcpAgent;
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Parser, Debug)]
#[command(name = "symposium-benchmark")]
#[command(about = "Benchmark harness for rust-crate-sources-proxy")]
struct Args {
    /// Benchmark to run (serde_from_value, etc.)
    #[arg(short, long)]
    benchmark: Option<String>,

    /// Directory to save raw output files
    #[arg(short, long, default_value = "benchmark-output")]
    output_dir: PathBuf,

    /// List available benchmarks
    #[arg(short, long)]
    list: bool,

    /// Enable logging for specific targets (comma-separated, e.g., "sacp,sacp_conductor")
    #[arg(long)]
    log: Option<String>,
}

struct Benchmark {
    name: &'static str,
    prompt: &'static str,
    expected: &'static str,
}

const BENCHMARKS: &[Benchmark] = &[Benchmark {
    name: "serde_from_value",
    prompt: "Please use the `rust_crate_query` tool from the `rust-crate-sources` MCP server \
             to research the signature of the serde_json::from_value API and describe what \
             inputs it accepts. Do not try to read files from disk - use the MCP tool.",
    expected: "The response should describe that serde_json::from_value takes a \
                   serde_json::Value and deserializes it into a type T. It should mention \
                   that it returns a Result<T, Error>.",
}];

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize tracing based on --log argument
    if let Some(log_targets) = &args.log {
        let filter =
            tracing_subscriber::EnvFilter::try_new(log_targets).expect("invalid log filter syntax");
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .init();
        tracing::info!("Logging enabled for: {}", log_targets);
    }

    // List benchmarks if requested
    if args.list {
        println!("Available benchmarks:");
        for benchmark in BENCHMARKS {
            println!("  - {}", benchmark.name);
        }
        return Ok(());
    }

    // Determine which benchmarks to run
    let benchmarks_to_run: Vec<&Benchmark> = if let Some(name) = &args.benchmark {
        BENCHMARKS.iter().filter(|b| b.name == name).collect()
    } else {
        BENCHMARKS.iter().collect()
    };

    if benchmarks_to_run.is_empty() {
        anyhow::bail!(
            "Benchmark '{}' not found. Use --list to see available benchmarks.",
            args.benchmark.unwrap()
        );
    }

    // Create output directory
    std::fs::create_dir_all(&args.output_dir)?;

    // Run benchmarks
    for benchmark in benchmarks_to_run {
        tracing::info!("Running benchmark: {}", benchmark.name);
        run_benchmark(benchmark, &args.output_dir).await?;
    }

    Ok(())
}

async fn run_benchmark(benchmark: &Benchmark, output_dir: &PathBuf) -> Result<()> {
    let research_prompt = benchmark.prompt;
    let expected_result = benchmark.expected;

    // Build Symposium agent with crate sources proxy
    let agent = AcpAgent::from_str("npx -y '@zed-industries/claude-code-acp'")?;
    let symposium = symposium_acp_proxy::Symposium::new()
        .sparkle(false)
        .trace_dir(".")
        .with_agent(agent);

    // Run prompt
    let response = yopo::prompt(symposium, research_prompt).await?;

    tracing::info!("Research response received: {} chars", response.len());

    // Validate response using another Claude Code instance
    tracing::info!("Validating response");

    let validation_result = yopo::prompt(
        AcpAgent::from_str("npx -y '@zed-industries/claude-code-acp'")?,
        &format!(
            "Compare this response to the expected result and respond with PASS or FAIL. \
         If FAIL, explain what's missing.\n\n\
         Expected: {}\n\n\
         Actual response:\n{}",
            expected_result, response
        ),
    )
    .await?;

    // Save outputs to files
    let prompt_file = output_dir.join(format!("{}_prompt.txt", benchmark.name));
    let response_file = output_dir.join(format!("{}_response.txt", benchmark.name));
    let validation_file = output_dir.join(format!("{}_validation.txt", benchmark.name));
    let expected_file = output_dir.join(format!("{}_expected.txt", benchmark.name));

    std::fs::write(&prompt_file, research_prompt)?;
    std::fs::write(&response_file, &response)?;
    std::fs::write(&validation_file, &validation_result)?;
    std::fs::write(&expected_file, expected_result)?;

    tracing::info!("Output saved to:");
    tracing::info!("  Prompt: {}", prompt_file.display());
    tracing::info!("  Response: {}", response_file.display());
    tracing::info!("  Expected: {}", expected_file.display());
    tracing::info!("  Validation: {}", validation_file.display());

    println!("\n=== BENCHMARK: {} ===", benchmark.name);
    println!("VALIDATION RESULT:\n{}", validation_result);
    println!("========================\n");

    Ok(())
}
