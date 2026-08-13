mod cli;
mod commands;
mod db;
mod error;
mod i18n;
mod model;
mod parser;
mod repo;
mod time;
mod tui;

use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    let cli = cli::Cli::parse();
    if let Some(cli::Command::Completions { shell }) = cli.command {
        cli::Cli::print_completions(shell);
        return Ok(());
    }
    let conn = db::conn::open()?;
    commands::run(cli.command.unwrap_or(cli::Command::Tui), &conn)
}
