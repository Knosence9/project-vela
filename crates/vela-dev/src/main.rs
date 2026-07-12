use clap::Parser;
use vela_dev::Cli;

fn main() -> std::process::ExitCode {
    Cli::parse().run()
}
