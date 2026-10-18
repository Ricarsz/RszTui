mod app;
mod asus;
mod cli;
mod config;
mod sys;
mod ui;
mod util;

use clap::Parser;
use cli::Cli;

fn main() {
    let cli = Cli::parse();

    if !util::is_root() {
        eprintln!("rogtui 需要 root 权限运行，请使用：sudo rogtui");
        std::process::exit(1);
    }

    let cfg = match config::Config::load() {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("读取配置失败：{err:#}");
            std::process::exit(1);
        }
    };

    let result = match cli.command {
        None => app::run(cfg),
        Some(cmd) => cli::run(cmd, cli.json),
    };

    if let Err(err) = result {
        eprintln!("错误：{err:#}");
        std::process::exit(1);
    }
}
