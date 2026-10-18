mod app;
mod asus;
mod cli;
mod config;
mod deps;
mod nm;
mod services;
mod sys;
mod ui;
mod util;

use clap::Parser;
use cli::Cli;

fn main() {
    // 恢复默认 SIGPIPE 行为，避免 `rsztui ... | head` 时 panic。
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    let cli = Cli::parse();

    let is_doctor = matches!(&cli.command, Some(cli::Cmd::Doctor));
    if !is_doctor && !util::is_root() {
        eprintln!("rsztui 需要 root 权限运行，请使用：sudo rsztui");
        std::process::exit(1);
    }

    let cfg = match config::Config::load() {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("读取配置失败：{err:#}");
            std::process::exit(1);
        }
    };

    let deps = deps::check();
    let result = match cli.command {
        None => app::run(cfg, deps),
        Some(cmd) => {
            if !matches!(cmd, cli::Cmd::Doctor)
                && let Some(warning) = deps.startup_warning()
            {
                eprintln!("{warning}");
            }
            cli::run(cmd, cli.json)
        }
    };

    if let Err(err) = result {
        eprintln!("错误：{err:#}");
        std::process::exit(1);
    }
}
