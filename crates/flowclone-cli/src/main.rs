//! flowclone — command-line interface for testing & debugging the engine.
//!
//! Usage:
//!   flowclone list-disks
//!   flowclone clone <source> <target> [--no-verify]
//!   flowclone verify <source> <target>
//!   flowclone version

use anyhow::Result;
use flowclone_core::{CloneEngine, CloneJob, CloneOptions};
use flowclone_disk::{Connection, DiskCatalogApi, Health};
use flowclone_report::ReportFormat;
use flowclone_verify::Verifier;
use std::path::PathBuf;
use std::time::Duration;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().cloned().unwrap_or_else(|| "help".into());
    let rest = &args[1..];

    match cmd.as_str() {
        "list-disks" | "ls" => list_disks(),
        "clone" => clone_cmd(rest).await,
        "verify" => verify_cmd(rest),
        "version" | "--version" | "-v" => {
            println!("flowclone {}", flowclone_core::VERSION);
            Ok(())
        }
        _ => {
            print_help();
            Ok(())
        }
    }
}

fn list_disks() -> Result<()> {
    let catalog = flowclone_disk::DiskCatalog::platform_default();
    let disks = catalog.list()?;
    if disks.is_empty() {
        println!("No disks found.");
        return Ok(());
    }
    for d in disks {
        println!(
            "{:<14} {:<8} {:>14}  {}  {}{}",
            d.device_path,
            d.bsd_name,
            humansize(d.total_bytes),
            d.model,
            fs_or_dash(&d.filesystem),
            badges(&d)
        );
    }
    Ok(())
}

async fn clone_cmd(args: &[String]) -> Result<()> {
    let (source, target) = match (args.first(), args.get(1)) {
        (Some(s), Some(t)) => (s.clone(), t.clone()),
        _ => {
            eprintln!("usage: flowclone clone <source> <target> [--no-verify]");
            std::process::exit(2);
        }
    };
    let verify = !args.iter().any(|a| a == "--no-verify");

    let engine = CloneEngine::new();
    let mut progress = engine.progress();
    // Background task: print progress snapshots as they arrive.
    let print_task = tokio::spawn(async move {
        loop {
            match progress.recv().await {
                Ok(p) => {
                    eprint!(
                        "\r{} [{:>5.1}%] {}/s  eta {:.0}s   ",
                        p.current_operation,
                        p.percent(),
                        humansize(p.write_speed),
                        p.eta_secs.unwrap_or(0.0)
                    );
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            }
        }
    });

    let options = CloneOptions {
        verify,
        ..Default::default()
    };
    let request = engine.resolve_request(&source, &target, &options)?;
    let job = CloneJob::new(request)?;

    let outcome = engine.run(job, options).await?;
    print_task.abort();
    eprintln!();

    println!(
        "copied {} in {:.1}s ({}/s avg); verify={}",
        humansize(outcome.copy.bytes_copied),
        outcome.copy.elapsed_secs,
        humansize(outcome.copy.average_speed),
        match &outcome.verify {
            Some(v) if v.matched => "PASS",
            Some(_) => "FAIL",
            None => "skipped",
        }
    );
    Ok(())
}

fn verify_cmd(args: &[String]) -> Result<()> {
    let (source, target) = match (args.first(), args.get(1)) {
        (Some(s), Some(t)) => (s.clone(), t.clone()),
        _ => {
            eprintln!("usage: flowclone verify <source> <target>");
            std::process::exit(2);
        }
    };
    let verifier = flowclone_verify::default_verifier();
    let total = std::fs::metadata(&source)?.len();
    let r = verifier.verify(&source, &target, total)?;
    println!(
        "matched={} blocks={} bytes={} elapsed={:.2}s",
        r.matched, r.blocks_checked, r.bytes_checked, r.elapsed_secs
    );
    if !r.matched {
        std::process::exit(1);
    }
    let _ = ReportFormat::Markdown; // placeholder use of report module
    Ok(())
}

fn print_help() {
    eprintln!("flowclone — {VERSION}\n");
    eprintln!("Commands:");
    eprintln!("  list-disks              List discovered disks");
    eprintln!("  clone <src> <tgt>       Clone src into tgt (raw)");
    eprintln!("  verify <src> <tgt>      Verify two devices match");
    eprintln!("  version                 Print version");
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

// ---- helpers -------------------------------------------------------------

fn humansize(n: u64) -> String {
    const UNITS: &[(&str, u64)] = &[
        ("TB", 1_000_000_000_000),
        ("GB", 1_000_000_000),
        ("MB", 1_000_000),
        ("KB", 1_000),
    ];
    for (u, scale) in UNITS {
        if n >= *scale {
            return format!("{:.0} {}", n as f64 / *scale as f64, u);
        }
    }
    format!("{n} B")
}

fn fs_or_dash(fs: &Option<String>) -> String {
    fs.clone().unwrap_or_else(|| "-".into())
}

fn badges(d: &flowclone_disk::DiskInfo) -> String {
    let mut s = String::new();
    if d.is_boot {
        s.push_str("[boot]");
    }
    if d.read_only {
        s.push_str("[ro]");
    }
    if d.encrypted {
        s.push_str("[enc]");
    }
    match d.health {
        Health::Healthy => s.push_str("[ok]"),
        Health::Warning => s.push_str("[warn]"),
        Health::Failing => s.push_str("[fail]"),
        Health::Unknown => {}
    }
    match d.connection {
        Connection::Usb => s.push_str("[usb]"),
        Connection::Thunderbolt => s.push_str("[tb]"),
        Connection::Internal => s.push_str("[int]"),
        _ => {}
    }
    s
}

// Sleep a tick to keep Clippy happy about unused Duration import in some builds.
#[allow(dead_code)]
fn _tick() {
    std::thread::sleep(Duration::from_micros(1));
}

// Avoid unused-import warnings on PathBuf (kept for future report path arg).
#[allow(dead_code)]
fn _path() -> PathBuf {
    PathBuf::new()
}
