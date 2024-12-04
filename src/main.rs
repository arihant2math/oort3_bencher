use std::path::Path;
use std::collections::HashMap;
use clap::Parser;
use oort_simulator::simulation::Code;
use oort_simulator::{scenario, simulation};
use oort_tools::AI;
use std::default::Default;
use std::fmt::{Display, Formatter};
use std::io::BufRead;
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use log::{debug, info, warn};
use oort_simulator::scenario::Status;
use oort_simulator::vm::builtin;
use rayon::iter::{ParallelIterator, IntoParallelRefIterator, IntoParallelIterator};
use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const OFF_BOLD: &str = "\x1b[21m";
const GREEN: &str = "\x1b[32m";
const BRIGHT_GREEN: &str = "\x1b[92m";
const RED: &str = "\x1b[31m";
const BRIGHT_RED: &str = "\x1b[91m";
const YELLOW: &str = "\x1b[33m";
const BRIGHT_YELLOW: &str = "\x1b[93m";
const BLUE: &str = "\x1b[34m";
const BRIGHT_BLUE: &str = "\x1b[94m";

#[derive(Clone, Parser, Debug)]
#[clap()]
struct Arguments {
    baseline_shortcode: String,
    new_shortcode: String,

    #[clap(short, long, default_value = "10")]
    rounds: u32,

    #[clap(short, long)]
    dev: bool,

    #[clap(long, default_value = "/tmp/oort-wasm-cache")]
    wasm_cache: Option<PathBuf>,

    scene_listing: String
}

#[derive(Clone, Default, Debug)]
struct Results {
    team0_wins: Vec<u32>,
    team1_wins: Vec<u32>,
    draws: Vec<u32>,
    times: Vec<f64>,
}

fn run_simulations(scenario_name: &str, codes: Vec<Code>, rounds: u32) -> Result<Results, String> {
    let seed_statuses: Vec<(u32, (Status, f64))> = (0..rounds)
        .into_iter()
        .map(|seed| (seed, run_simulation(scenario_name, seed, codes.clone())))
        .collect();
    info!("Simulation complete");
    let mut results: Results = Default::default();
    debug!("Processing results");
    for (seed, (status, time)) in seed_statuses {
        match status {
            Status::Victory { team: 0 } => results.team0_wins.push(seed),
            Status::Victory { team: 1 } => results.team1_wins.push(seed),
            Status::Victory { team: s } => return Err(format!("Invalid team {}", s)),
            Status::Draw => results.draws.push(seed),
            Status::Failed => results.team1_wins.push(seed),
            Status::Running => return Err("Scenario should not be running".to_string()),
        }
        results.times.push(time);
    }
    Ok(results)
}

fn run_simulation(scenario_name: &str, seed: u32, codes: Vec<Code>) -> (Status, f64) {
    debug!("Running simulation {scenario_name} at seed {seed}");
    let mut sim = simulation::Simulation::new(scenario_name, seed, &codes);
    while sim.status() == Status::Running {
        sim.step();
        if sim.tick() == scenario::MAX_TICKS {
            warn!("Simulation {scenario_name} at seed {seed} exceeding max ticks");
            return (Status::Failed, sim.score_time());
        }
    }
    (sim.status(), sim.score_time())
}

#[derive(Clone)]
struct BenchmarkResults {
    scene: String,
    baseline: Results,
    new: Results,
}

impl Display for BenchmarkResults {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let win_change = self.new.team0_wins.len() as i32 - self.baseline.team0_wins.len() as i32;
        if win_change > 0 {
            write!(f, "{BOLD}Win change{RESET} {BOLD}{BRIGHT_GREEN}+{}{RESET}", win_change)
        } else if win_change == 0 {
            write!(f, "{BOLD}Win change{RESET} {GREEN}None{RESET}")
        } else {
            write!(f, "{BOLD}Win change{RESET} {BOLD}{BRIGHT_RED}{}{RESET}", win_change)
        }?;
        write!(f, " ({} -> {})\n", self.baseline.team0_wins.len(), self.new.team0_wins.len())?;
        let avg_time = |results: &Results| -> f64 {
            results.times.iter().sum::<f64>() / results.times.len() as f64
        };
        let baseline_avg_time = avg_time(&self.baseline);
        let new_avg_time = avg_time(&self.new);
        if new_avg_time < baseline_avg_time {
            write!(f, "{BOLD}Avg time change{RESET} {BOLD}{BRIGHT_GREEN}-{:.3}{RESET}", baseline_avg_time - new_avg_time)
        } else if new_avg_time == baseline_avg_time {
            write!(f, "{BOLD}Avg time change{RESET} {GREEN}None{RESET}")
        } else {
            write!(f, "{BOLD}Avg time change{RESET} {BOLD}{BRIGHT_RED}{:.3}{RESET}", new_avg_time - baseline_avg_time)
        }?;
        write!(f, " ({:.3} -> {:.3})", baseline_avg_time, new_avg_time)?;
        Ok(())
    }
}

fn run_simulations_packaged(args: &Arguments, scene: &str, player: &AI, enemy: &AI) -> Result<Results, String> {
    info!("Running Scene: {scene}");
    scenario::load_safe(scene).expect(&format!("Unknown scenario {scene}"));
    info!("Compiling AIs");

    info!("Running simulations");
    let results = run_simulations(scene, vec![player.compiled_code.clone(), enemy.compiled_code.clone()], args.rounds)?;

    Ok(results)
}

fn run_benchmark(args: Arguments, scene: String, enemy: Code, compiled_baseline: &AI, compiled_new: &AI) -> BenchmarkResults {
    let mut compiler = oort_compiler::Compiler::new();
    let src = match enemy {
        Code::Rust(src) => src,
        Code::Builtin(name) => {
            match builtin::load_source(&name) {
                Ok(src) => match src {
                    Code::Rust(src) => src,
                    _ => panic!("Invalid builtin code"),
                },
                Err(e) => panic!("Invalid builtin code: {e}"),
            }
        }
        _ => panic!("Invalid code type"),
    };
    let wasm = compiler.compile(&src).unwrap();
    let enemy_ai = AI {
        name: "Enemy".to_string(),
        source_code: src,
        compiled_code: Code::Wasm(wasm),
    };
    let res = vec![compiled_baseline, compiled_new].into_par_iter().map(|p| {
        run_simulations_packaged(&args, &scene, p, &enemy_ai).unwrap()
    }).collect::<Vec<Results>>();
    let base_results = res[0].clone();
    let new_results = res[1].clone();
    BenchmarkResults {
        scene,
        baseline: base_results,
        new: new_results,

    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    env_logger::Builder::from_env(env_logger::Env::default().filter_or("benchmark", "warn"))
        .init();

    let args = Arguments::parse();

    let mut scene_mapping = HashMap::new();
    let scenes: Vec<String> = if Path::new(&args.scene_listing).is_file() {
        let scene_file = std::fs::File::open(&args.scene_listing)?;
        std::io::BufReader::new(scene_file)
            .lines()
            .map(|line| line.unwrap())
            .filter(|line| !line.starts_with('#'))
            .map(|line| line.trim().to_string())
            .collect()
    } else {
        args.scene_listing
            .split(',')
            .map(|s| s.to_string())
            .collect()
    };
    for scene in scenes {
        let scenario = scenario::load_safe(&scene).expect(&format!("Unknown scenario {scene}"));
        scene_mapping.insert(scene.to_string(), scenario.initial_code()[1].clone());
    }

    println!("{BRIGHT_BLUE}Compiling inputted AIs{RESET}");
    let mut compiler = oort_compiler::Compiler::new();
    let src = std::fs::read_to_string(&args.baseline_shortcode).unwrap();
    let wasm = compiler.compile(&src).unwrap();
    let baseline = AI {
        name: args.baseline_shortcode.to_string(),
        source_code: src,
        compiled_code: Code::Wasm(wasm),
    };
    let src = std::fs::read_to_string(&args.new_shortcode).unwrap();
    let wasm = compiler.compile(&src).unwrap();
    let new = AI {
        name: args.new_shortcode.to_string(),
        source_code: src,
        compiled_code: Code::Wasm(wasm),
    };

    let converted_scene_mapping: Vec<(String, Code)> = scene_mapping.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    println!("{BRIGHT_BLUE}Running Benchmarks{RESET}");
    let completed_num = AtomicUsize::new(0);
    let total = converted_scene_mapping.len();
    let results = converted_scene_mapping.par_iter().map(|(scene, enemy)| {
        let args = args.clone();
        let results = run_benchmark(args, scene.clone(), enemy.clone(), &baseline, &new);
        completed_num.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let completed = completed_num.load(std::sync::atomic::Ordering::SeqCst);
        println!("{BRIGHT_BLUE}Completed {completed}/{total} benchmarks{RESET}", completed = completed, total = total);
        results
    }).collect::<Vec<BenchmarkResults>>();
    println!("{BRIGHT_BLUE}Results{RESET}");

    for result in results {
        println!("{BRIGHT_BLUE}Results for {BOLD}{scene}{OFF_BOLD}", scene = result.scene);
        println!("{}", result);
    }

    Ok(())
}
