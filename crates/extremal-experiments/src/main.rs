//! CLI for running experiment benchmarks.

use clap::{Parser, Subcommand};
use extremal_experiments::harness::{compare_strategies, print_results, standard_problems};

#[derive(Parser)]
#[command(name = "extremal-experiments")]
#[command(about = "Benchmark and compare Extremal search strategies")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Compare all strategies on a problem.
    Compare {
        /// Target vertex count (5, 17, 25, or 43 for standard problems).
        #[arg(long)]
        n: u32,

        /// Iteration budget per round.
        #[arg(long, default_value = "100000")]
        budget: u64,

        /// Number of random seeds to average over.
        #[arg(long, default_value = "5")]
        seeds: u32,

        /// Only run a specific strategy (by id).
        #[arg(long)]
        strategy: Option<String>,
    },

    /// Run all standard problems.
    Suite {
        /// Iteration budget per round.
        #[arg(long, default_value = "100000")]
        budget: u64,

        /// Number of random seeds to average over.
        #[arg(long, default_value = "5")]
        seeds: u32,
    },

    /// List available strategies.
    List,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Compare {
            n,
            budget,
            seeds,
            strategy,
        } => {
            let problems = standard_problems();
            let problem = problems.iter().find(|p| p.n == n).unwrap_or_else(|| {
                eprintln!(
                    "No standard problem for n={}. Available: {}",
                    n,
                    problems
                        .iter()
                        .map(|p| p.n.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                std::process::exit(1);
            });

            let all = extremal_experiments::all_strategies();
            let strategies: Vec<_> = if let Some(ref id) = strategy {
                all.into_iter().filter(|s| s.id() == id).collect()
            } else {
                all
            };

            if strategies.is_empty() {
                eprintln!("No matching strategy found");
                std::process::exit(1);
            }

            println!(
                "Comparing {} strategies on {} (budget={}, seeds={})",
                strategies.len(),
                problem.name,
                budget,
                seeds
            );

            let results = compare_strategies(&strategies, problem, budget, seeds);
            print_results(problem, &results);
        }

        Commands::Suite { budget, seeds } => {
            let strategies = extremal_experiments::all_strategies();
            let problems = standard_problems();

            println!(
                "Running {} strategies on {} problems (budget={}, seeds={})\n",
                strategies.len(),
                problems.len(),
                budget,
                seeds
            );

            for problem in &problems {
                let results = compare_strategies(&strategies, problem, budget, seeds);
                print_results(problem, &results);
            }
        }

        Commands::List => {
            println!("Production strategies:");
            for s in extremal_strategies::default_strategies() {
                println!("  {} — {}", s.id(), s.name());
            }
            println!("\nExperiment strategies:");
            for s in extremal_experiments::experiment_strategies() {
                println!("  {} — {}", s.id(), s.name());
            }
        }
    }
}
