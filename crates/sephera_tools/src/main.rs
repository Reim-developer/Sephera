use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use sephera_tools::{
    benchmark_corpus::{
        default_benchmark_corpus_path, default_benchmark_dataset_names,
        generate_benchmark_corpus,
    },
    language_data::{
        default_generated_language_data_path, default_language_config_path,
        generate_language_data_file,
    },
};

#[derive(Debug, Parser)]
#[command(
    name = "sephera_tools",
    version,
    about = "Internal tooling for Sephera code generation and benchmark fixtures"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Generate Rust language metadata from YAML
    GenerateLanguageData {
        /// Input YAML configuration path
        #[arg(long, default_value_os_t = default_language_config_path())]
        input: PathBuf,

        /// Generated Rust output path
        #[arg(long, default_value_os_t = default_generated_language_data_path())]
        output: PathBuf,
    },

    /// Generate a deterministic benchmark corpus under the chosen directory
    GenerateBenchmarkCorpus {
        /// Output directory for the synthetic corpus
        #[arg(long, default_value_os_t = default_benchmark_corpus_path())]
        output: PathBuf,

        /// Remove the output directory first when it already exists
        #[arg(long)]
        clean: bool,

        /// Dataset names to generate. Defaults to the standard suite when omitted.
        #[arg(long, value_enum, num_args = 1..)]
        datasets: Vec<BenchmarkDatasetArg>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum BenchmarkDatasetArg {
    Small,
    Medium,
    Large,
    #[value(name = "extra-large")]
    ExtraLarge,
}

impl BenchmarkDatasetArg {
    const fn as_name(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
            Self::ExtraLarge => "extra-large",
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::GenerateLanguageData { input, output } => {
            generate_language_data_file(&input, &output)?;
        }
        Commands::GenerateBenchmarkCorpus {
            output,
            clean,
            datasets,
        } => {
            let selected_datasets = if datasets.is_empty() {
                default_benchmark_dataset_names().to_vec()
            } else {
                datasets.iter().map(|dataset| dataset.as_name()).collect()
            };
            generate_benchmark_corpus(&output, clean, &selected_datasets)?;
        }
    }

    Ok(())
}
