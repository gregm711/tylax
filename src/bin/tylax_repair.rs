//! Tylax auto-repair pipeline for LaTeX -> Typst conversions.

#[cfg(feature = "cli")]
use clap::Parser;
use std::fs;
use std::io::{self, Read, Write};
use tylax::core::latex2typst::{latex_math_to_typst_with_report, latex_to_typst_with_report};
use tylax::utils::loss::ConversionReport;
use tylax::utils::repair::{maybe_repair_latex_to_typst, AiRepairConfig};

#[cfg(feature = "cli")]
#[derive(Parser)]
#[command(name = "tylax-repair")]
#[command(author = "SciPenAI")]
#[command(version)]
#[command(about = "Auto-repair LaTeX -> Typst conversion using an AI hook", long_about = None)]
struct Args {
    /// Input LaTeX file path (reads from stdin if not provided)
    input: Option<String>,

    /// Output file path (writes to stdout if not provided)
    #[arg(short, long)]
    output: Option<String>,

    /// Treat input as full document (default: math-only)
    #[arg(short = 'f', long)]
    full_document: bool,

    /// Write a loss report JSON to this path
    #[arg(long)]
    loss_log: Option<String>,

    /// Enable AI auto-repair (requires --ai-cmd or TYLAX_AI_CMD)
    #[arg(long)]
    auto_repair: bool,

    /// Command to invoke for AI repair (reads JSON on stdin, writes Typst on stdout)
    #[arg(long)]
    ai_cmd: Option<String>,

    /// Allow AI output even if it does not reduce loss markers
    #[arg(long)]
    allow_no_gain: bool,
}


fn main() -> io::Result<()> {
    let args = Args::parse();

    let (input, filename) = match args.input {
        Some(ref path) => (fs::read_to_string(path)?, Some(path.clone())),
        None => {
            let mut buffer = String::new();
            io::stdin().read_to_string(&mut buffer)?;
            (buffer, None)
        }
    };

    let ConversionReport { content, report } = if args.full_document {
        latex_to_typst_with_report(&input)
    } else {
        latex_math_to_typst_with_report(&input)
    };

    if let Some(path) = &args.loss_log {
        let serialized = serde_json::to_string_pretty(&report)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
        fs::write(path, serialized)?;
    }

    let repair_config = AiRepairConfig {
        auto_repair: args.auto_repair,
        ai_cmd: args.ai_cmd.clone(),
        allow_no_gain: args.allow_no_gain,
    };

    let final_output = maybe_repair_latex_to_typst(&input, &content, &report, &repair_config);

    match args.output {
        Some(path) => {
            let mut file = fs::File::create(&path)?;
            writeln!(file, "{}", final_output)?;
            if let Some(name) = filename {
                eprintln!("✓ Repaired output written to: {} (from {})", path, name);
            } else {
                eprintln!("✓ Repaired output written to: {}", path);
            }
        }
        None => {
            println!("{}", final_output);
        }
    }

    Ok(())
}
