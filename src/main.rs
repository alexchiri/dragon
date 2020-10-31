use std::path::{PathBuf, Path};

use structopt::StructOpt;
use anyhow::{Context, Result};
use log::{info, warn, debug, trace};

#[derive(Debug, StructOpt)]
#[structopt(name = "dragon", about = "A CLI tool that manages project-dev-palace WSL2 VMs.")]
struct Args {
    #[structopt(short = "c", long, parse(from_os_str))]
    dockerwsl: Option<PathBuf>,

    #[structopt(flatten)]
    verbose: clap_verbosity_flag::Verbosity,
}

fn main() -> Result<()> {
    let args = Args::from_args();
    simple_logger::init_with_level(args.verbose.log_level().unwrap());
    println!("Hello, world!");
    Ok(())
}
