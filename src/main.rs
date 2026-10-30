use anyhow::{bail, Context, Result};
use clap::Parser;
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm, Select};
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[derive(Parser, Debug)]
#[command(name = "apk-adb-tool")]
#[command(about = "Search installed apps on Android device by name, pull APK and (re)install via ADB", long_about = None)]
struct Args {
    /// Search term (package name or part of it). If omitted, will ask interactively.
    #[arg(short, long)]
    search: Option<String>,

    /// Device serial (if multiple devices connected)
    #[arg(short = 's', long)]
    serial: Option<String>,

    /// Only third-party (user) apps
    #[arg(short = '3', long = "third-party")]
    third_party: bool,

    /// Only pull APK, do not reinstall
    #[arg(long)]
    pull_only: bool,

    /// Output directory for pulled APKs (default: current dir)
    #[arg(short, long, default_value = ".")]
    output: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Check adb is available
    let adb = which::which("adb").context(
        "ADB not found in PATH. Install Android Platform Tools and add them to PATH.",
    )?;
    println!("{} ADB found: {}", "✓".green(), adb.display());

    // Check device(s)
    let devices = run_adb(&args, &["devices"])?;
    let online: Vec<_> = devices
        .lines()
        .skip(1)
        .filter(|l| l.contains("\tdevice"))
        .collect();
    if online.is_empty() {
        bail!("No online Android device found. Enable USB debugging and connect the phone.");
    }
    if online.len() > 1 && args.serial.is_none() {
        println!(
            "{} Multiple devices detected. Use -s <serial> to select one.",
            "!".yellow()
        );
        for d in &online {
            println!("  {}", d.trim());
        }
        bail!("Specify device with -s");
    }
    println!("{} Device(s) online: {}", "✓".green(), online.len());

    // Get search term
    let search = match args.search {
        Some(s) => s,
        None => {
            dialoguer::Input::<String>::with_theme(&ColorfulTheme::default())
                .with_prompt("Enter app name / package part to search")
                .interact_text()?
        }
    };
    let search_lower = search.to_lowercase();
    if search_lower.is_empty() {
        bail!("Search term cannot be empty");
    }

    // List packages
    let mut list_cmd = vec!["shell", "pm", "list", "packages"];
    if args.third_party {
        list_cmd.push("-3");
    }
    let packages_raw = run_adb(&args, &list_cmd)?;
    let mut matches: Vec<String> = packages_raw
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if !line.starts_with("package:") {
                return None;
            }
            let pkg = line.trim_start_matches("package:").trim();
            if pkg.to_lowercase().contains(&search_lower) {
                Some(pkg.to_string())
            } else {
                None
            }
        })
        .collect();

    matches.sort();
    matches.dedup();

    if matches.is_empty() {
        println!(
            "{} No packages matching '{}' found.",
            "✗".red(),
            search.cyan()
        );
        return Ok(());
    }

    println!(
        "\n{} Found {} matching package(s) for '{}':",
        "→".blue(),
        matches.len().to_string().cyan(),
        search.cyan()
    );

    // Interactive select
    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Select package to download APK + install")
        .items(&matches)
        .default(0)
        .interact()?;

    let package = &matches[selection];
    println!("\n{} Selected: {}", "→".blue(), package.green());

    // Get APK path on device
    let path_out = run_adb(&args, &["shell", "pm", "path", package])?;
    let apk_path_on_device = path_out
        .lines()
        .find(|l| l.starts_with("package:"))
        .map(|l| l.trim_start_matches("package:").trim().to_string())
        .context("Could not get APK path for package")?;

    println!("{} Device APK path: {}", "→".blue(), apk_path_on_device);

    // Local filename
    let safe_name = package.replace('.', "_");
    let local_apk = args.output.join(format!("{}.apk", safe_name));
    println!("{} Pulling to: {}", "↓".yellow(), local_apk.display());

    // Pull
    let pull_status = run_adb_status(
        &args,
        &["pull", &apk_path_on_device, local_apk.to_str().unwrap()],
    )?;
    if !pull_status.success() {
        bail!("Failed to pull APK. Does the device allow access to the path? Try with root or check permissions.");
    }
    println!("{} APK downloaded successfully: {}", "✓".green(), local_apk.display());

    if args.pull_only {
        println!("{} Pull-only mode, skipping install.", "i".blue());
        return Ok(());
    }

    // Confirm install
    let do_install = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt(format!(
            "Reinstall {} on the device? (will use -r)",
            package
        ))
        .default(true)
        .interact()?;

    if !do_install {
        println!("Skipped install.");
        return Ok(());
    }

    println!("{} Installing...", "↑".yellow());
    let install_status = run_adb_status(
        &args,
        &[
            "install",
            "-r", // replace existing
            "-t", // allow test packages if needed
            local_apk.to_str().unwrap(),
        ],
    )?;

    if install_status.success() {
        println!("{} Successfully installed {}!", "✓".green(), package.green());
    } else {
        // Try without -t
        println!("{} First attempt failed, retrying without -t...", "!".yellow());
        let retry = run_adb_status(
            &args,
            &["install", "-r", local_apk.to_str().unwrap()],
        )?;
        if retry.success() {
            println!("{} Successfully installed {}!", "✓".green(), package.green());
        } else {
            bail!("Install failed. Check logs above or try manually: adb install -r {}", local_apk.display());
        }
    }

    Ok(())
}

fn run_adb(args: &Args, cmd_args: &[&str]) -> Result<String> {
    let mut full = Vec::new();
    if let Some(serial) = &args.serial {
        full.push("-s");
        full.push(serial.as_str());
    }
    full.extend_from_slice(cmd_args);

    let output = Command::new("adb")
        .args(&full)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .context("Failed to execute adb")?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        bail!("adb {:?} failed: {}", full, err.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn run_adb_status(args: &Args, cmd_args: &[&str]) -> Result<std::process::ExitStatus> {
    let mut full = Vec::new();
    if let Some(serial) = &args.serial {
        full.push("-s");
        full.push(serial.as_str());
    }
    full.extend_from_slice(cmd_args);

    let status = Command::new("adb")
        .args(&full)
        .status()
        .context("Failed to execute adb")?;

    Ok(status)
}
