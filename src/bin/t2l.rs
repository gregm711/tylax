//! Tylax CLI - High-performance bidirectional LaTeX ↔ Typst converter

#[cfg(feature = "cli")]
use clap::{Parser, Subcommand, ValueEnum};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use tylax::{
    convert_auto, convert_auto_document, detect_format,
    diagnostics::{check_latex, format_diagnostics},
    latex_document_to_typst, latex_math_to_typst_with_report, latex_to_typst,
    latex_to_typst_with_report,
    tikz::{convert_cetz_to_tikz, convert_tikz_to_cetz, is_cetz_code},
    typst_document_to_latex, typst_to_latex, typst_to_latex_ir, typst_to_latex_ir_with_report,
    utils::repair::{AiRepairConfig, maybe_repair_typst_to_latex},
    utils::loss::{LossKind, LossRecord, LossReport, LOSS_MARKER_PREFIX},
    utils::latex_analysis::metrics_source as latex_metrics_source,
    utils::typst_analysis::metrics_source as typst_metrics_source,
};
use tylax::core::latex2typst::utils::{
    collect_bibliography_entries, collect_graphicspath_entries, collect_includegraphics_paths,
    expand_latex_inputs, sanitize_bibtex_content,
};

#[cfg(feature = "cli")]
#[derive(Parser)]
#[command(name = "t2l")]
#[command(author = "SciPenAI")]
#[command(version)]
#[command(about = "Tylax - High-performance bidirectional LaTeX ↔ Typst converter", long_about = None)]
struct Cli {
    /// Subcommand to run
    #[command(subcommand)]
    command: Option<Commands>,

    /// Input file path (reads from stdin if not provided)
    input_file: Option<String>,

    /// Output file path (writes to stdout if not provided)
    #[arg(short, long)]
    output: Option<String>,

    /// Conversion direction
    #[arg(short, long, value_enum, default_value_t = Direction::Auto)]
    direction: Direction,

    /// Full document mode (convert entire document, not just math)
    #[arg(short = 'f', long)]
    full_document: bool,

    /// Pretty print the output
    #[arg(short, long)]
    pretty: bool,

    /// Enable AI auto-repair for LaTeX -> Typst conversions
    #[arg(long)]
    auto_repair: bool,

    /// Command to invoke for AI repair (reads JSON on stdin, writes Typst on stdout)
    #[arg(long)]
    ai_cmd: Option<String>,

    /// Write a loss report JSON to this path (LaTeX -> Typst only)
    #[arg(long)]
    loss_log: Option<String>,

    /// Write a post-repair report JSON to this path
    #[arg(long)]
    post_repair_log: Option<String>,

    /// Allow AI output even if it does not reduce loss markers
    #[arg(long)]
    allow_no_gain: bool,

    /// Use the IR-based Typst → LaTeX pipeline
    #[arg(long)]
    ir: bool,

    /// Detect and print the input format without converting
    #[arg(long)]
    detect: bool,

    /// Check mode - analyze LaTeX for potential issues without converting
    #[arg(long)]
    check: bool,

    /// Use colored output (for check mode)
    #[arg(long, default_value_t = true)]
    color: bool,
}

#[cfg(feature = "cli")]
#[derive(Subcommand)]
enum Commands {
    /// Check LaTeX for potential conversion issues
    Check {
        /// Input file to check
        input: Option<String>,

        /// Disable colored output
        #[arg(long)]
        no_color: bool,
    },

    /// Convert a file (default action)
    Convert {
        /// Input file path
        input: Option<String>,

        /// Output file path
        #[arg(short, long)]
        output: Option<String>,

        /// Conversion direction
        #[arg(short, long, value_enum, default_value_t = Direction::Auto)]
        direction: Direction,

        /// Full document mode
        #[arg(short = 'f', long)]
        full_document: bool,

        /// Use the IR-based Typst → LaTeX pipeline
        #[arg(long)]
        ir: bool,

        /// Enable AI auto-repair for LaTeX -> Typst conversions
        #[arg(long)]
        auto_repair: bool,

        /// Command to invoke for AI repair (reads JSON on stdin, writes Typst on stdout)
        #[arg(long)]
        ai_cmd: Option<String>,

        /// Write a loss report JSON to this path (LaTeX -> Typst only)
        #[arg(long)]
        loss_log: Option<String>,

        /// Write a post-repair report JSON to this path
        #[arg(long)]
        post_repair_log: Option<String>,

        /// Allow AI output even if it does not reduce loss markers
        #[arg(long)]
        allow_no_gain: bool,
    },

    /// Convert TikZ to CeTZ or vice versa
    Tikz {
        /// Input file containing TikZ or CeTZ code
        input: Option<String>,

        /// Output file path
        #[arg(short, long)]
        output: Option<String>,

        /// Direction (auto-detected by default)
        #[arg(short, long, value_enum, default_value_t = TikzDirection::Auto)]
        direction: TikzDirection,
    },

    /// Batch convert multiple files
    Batch {
        /// Input directory or glob pattern
        input: String,

        /// Output directory
        #[arg(short, long)]
        output_dir: String,

        /// Conversion direction
        #[arg(short, long, value_enum, default_value_t = Direction::L2t)]
        direction: Direction,

        /// Full document mode
        #[arg(short = 'f', long)]
        full_document: bool,

        /// File extension for output files
        #[arg(short, long)]
        extension: Option<String>,

        /// Use the IR-based Typst → LaTeX pipeline
        #[arg(long)]
        ir: bool,
    },

    /// Show version and feature info
    Info,
}

#[cfg(feature = "cli")]
#[derive(Clone, ValueEnum)]
enum TikzDirection {
    /// Auto-detect based on content
    Auto,
    /// TikZ to CeTZ
    TikzToCetz,
    /// CeTZ to TikZ
    CetzToTikz,
}

#[cfg(feature = "cli")]
#[derive(Clone, ValueEnum)]
enum Direction {
    /// Auto-detect based on file extension or content
    Auto,
    /// LaTeX to Typst
    L2t,
    /// Typst to LaTeX
    T2l,
}

#[cfg(feature = "cli")]
fn main() -> io::Result<()> {
    let cli = Cli::parse();

    // Handle subcommands first
    if let Some(cmd) = cli.command {
        return handle_subcommand(cmd);
    }

    // Read input
    let (mut input, filename) = match cli.input_file {
        Some(ref path) => (fs::read_to_string(path)?, Some(path.clone())),
        None => {
            let mut buffer = String::new();
            io::stdin().read_to_string(&mut buffer)?;
            (buffer, None)
        }
    };

    // If detect mode, just print format and exit
    if cli.detect {
        let format = detect_format(&input);
        println!("{}", format);
        return Ok(());
    }

    // If check mode, analyze and report issues
    if cli.check {
        let result = check_latex(&input);
        let output = format_diagnostics(&result, cli.color);
        println!("{}", output);

        // Exit with error code if there are errors
        if result.has_errors() {
            std::process::exit(1);
        }
        return Ok(());
    }

    // Determine direction
    let direction = match cli.direction {
        Direction::Auto => {
            if let Some(ref name) = filename {
                if name.ends_with(".typ") {
                    Direction::T2l
                } else if name.ends_with(".tex") {
                    Direction::L2t
                } else {
                    // Use content-based detection
                    let format = detect_format(&input);
                    if format == "latex" {
                        Direction::L2t
                    } else {
                        Direction::T2l
                    }
                }
            } else {
                // Use content-based detection
                let format = detect_format(&input);
                if format == "latex" {
                    Direction::L2t
                } else {
                    Direction::T2l
                }
            }
        }
        d => d,
    };

    let mut bib_entries: Vec<String> = Vec::new();
    let mut bib_base_dir: Option<std::path::PathBuf> = None;
    let mut graphic_paths: Vec<String> = Vec::new();
    let mut graphic_dirs: Vec<String> = Vec::new();
    let mut graphics_base_dir: Option<PathBuf> = None;

    if matches!(direction, Direction::L2t) && cli.full_document {
        if let Some(path) = filename.as_ref() {
            if let Some(parent) = Path::new(path).parent() {
                input = expand_latex_inputs(&input, parent);
                bib_entries = collect_bibliography_entries(&input);
                if bib_entries.is_empty() {
                    bib_entries = collect_bibliography_entries_with_includes(&input, parent);
                }
                bib_base_dir = Some(parent.to_path_buf());
                graphic_dirs = collect_graphicspath_entries(&input);
                graphic_paths = collect_includegraphics_paths(&input);
                graphics_base_dir = Some(parent.to_path_buf());
            }
        }
    }

    let repair_config = AiRepairConfig {
        auto_repair: cli.auto_repair,
        ai_cmd: cli.ai_cmd.clone(),
        allow_no_gain: cli.allow_no_gain,
    };
    let mut loss_report: Option<LossReport> = None;
    let mut post_report: Option<LossReport> = None;

    // Convert
    let mut result = if cli.full_document {
        match direction {
            Direction::L2t => {
                if cli.auto_repair || cli.loss_log.is_some() || cli.post_repair_log.is_some() {
                    let report = latex_to_typst_with_report(&input);
                    loss_report = Some(report.report.clone());
                    let repaired = tylax::utils::repair::maybe_repair_latex_to_typst(
                        &input,
                        &report.content,
                        &report.report,
                        &repair_config,
                    );
                    if cli.post_repair_log.is_some() {
                        post_report = Some(build_post_report_typst(&repaired));
                    }
                    repaired
                } else {
                    latex_document_to_typst(&input)
                }
            }
            Direction::T2l => {
                let use_ir = cli.ir
                    || cli.auto_repair
                    || cli.loss_log.is_some()
                    || cli.post_repair_log.is_some();
                if cli.auto_repair || cli.loss_log.is_some() || cli.post_repair_log.is_some() {
                    let report = typst_to_latex_ir_with_report(&input, true);
                    loss_report = Some(report.report.clone());
                    let repaired = maybe_repair_typst_to_latex(
                        &input,
                        &report.content,
                        &report.report,
                        &repair_config,
                    );
                    if cli.post_repair_log.is_some() {
                        post_report = Some(build_post_report_latex(&repaired));
                    }
                    repaired
                } else if use_ir {
                    typst_to_latex_ir(&input, true)
                } else {
                    typst_document_to_latex(&input)
                }
            }
            Direction::Auto => convert_auto_document(&input).0,
        }
    } else {
        match direction {
            Direction::L2t => {
                if cli.auto_repair || cli.loss_log.is_some() || cli.post_repair_log.is_some() {
                    let report = latex_math_to_typst_with_report(&input);
                    loss_report = Some(report.report.clone());
                    let repaired = tylax::utils::repair::maybe_repair_latex_to_typst(
                        &input,
                        &report.content,
                        &report.report,
                        &repair_config,
                    );
                    if cli.post_repair_log.is_some() {
                        post_report = Some(build_post_report_typst(&repaired));
                    }
                    repaired
                } else {
                    latex_to_typst(&input)
                }
            }
            Direction::T2l => {
                let use_ir = cli.ir
                    || cli.auto_repair
                    || cli.loss_log.is_some()
                    || cli.post_repair_log.is_some();
                if cli.auto_repair || cli.loss_log.is_some() || cli.post_repair_log.is_some() {
                    let report = typst_to_latex_ir_with_report(&input, false);
                    loss_report = Some(report.report.clone());
                    let repaired = maybe_repair_typst_to_latex(
                        &input,
                        &report.content,
                        &report.report,
                        &repair_config,
                    );
                    if cli.post_repair_log.is_some() {
                        post_report = Some(build_post_report_latex(&repaired));
                    }
                    repaired
                } else if use_ir {
                    typst_to_latex_ir(&input, false)
                } else {
                    typst_to_latex(&input)
                }
            }
            Direction::Auto => convert_auto(&input).0,
        }
    };

    if let (Some(path), Some(report)) = (cli.loss_log.as_ref(), loss_report.as_ref()) {
        let serialized = serde_json::to_string_pretty(report)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
        fs::write(path, serialized)?;
    }
    if let (Some(path), Some(report)) = (cli.post_repair_log.as_ref(), post_report.as_ref()) {
        let serialized = serde_json::to_string_pretty(report)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
        fs::write(path, serialized)?;
    }

    // Pretty print if requested
    if !bib_entries.is_empty() {
        if let Some(output_path) = cli.output.as_ref() {
            let out_dir = Path::new(output_path)
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            if let Some(base_dir) = bib_base_dir.as_ref() {
                if let Ok(mapping) = sanitize_bibliography_files(&bib_entries, base_dir, &out_dir) {
                    result = rewrite_bibliography_paths(&result, &mapping);
                    if !result.contains("#bibliography") {
                        let mut mapped: Vec<String> = Vec::new();
                        for entry in &bib_entries {
                            let mut name = entry.clone();
                            if !name.ends_with(".bib") {
                                name.push_str(".bib");
                            }
                            if let Some(mapped_name) = mapping.get(&name) {
                                mapped.push(mapped_name.clone());
                            }
                        }
                        if !mapped.is_empty() {
                            mapped.sort();
                            mapped.dedup();
                            let quoted: Vec<String> =
                                mapped.into_iter().map(|s| format!("\"{}\"", s)).collect();
                            if quoted.len() == 1 {
                                result.push_str("\n#bibliography(");
                                result.push_str(&quoted.join(", "));
                                result.push_str(")\n");
                            } else {
                                result.push_str("\n#bibliography((");
                                result.push_str(&quoted.join(", "));
                                result.push_str("))\n");
                            }
                        }
                    }
                }
            }
        }
    }
    if let (Some(output_path), Some(base_dir)) = (cli.output.as_ref(), graphics_base_dir.as_ref()) {
        let out_dir = Path::new(output_path)
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        if !graphic_paths.is_empty() {
            if let Ok(mapping) =
                copy_graphics_assets(&graphic_paths, &graphic_dirs, base_dir, &out_dir)
            {
                result = rewrite_image_paths(&result, &mapping);
            }
        }
        result = rewrite_extensionless_images(&result, &out_dir);
    }

    let result = if cli.pretty {
        pretty_print(&result)
    } else {
        result
    };

    // Output
    match cli.output {
        Some(path) => {
            let mut file = fs::File::create(&path)?;
            writeln!(file, "{}", result)?;
            eprintln!("✓ Output written to: {}", path);
        }
        None => {
            println!("{}", result);
        }
    }

    Ok(())
}

#[cfg(feature = "cli")]
fn sanitize_bibliography_files(
    entries: &[String],
    base_dir: &Path,
    out_dir: &Path,
) -> io::Result<std::collections::HashMap<String, String>> {
    let mut mapping = std::collections::HashMap::new();
    let mut merged_entries: Vec<(String, String)> = Vec::new();
    for entry in entries {
        let mut name = entry.clone();
        if !name.ends_with(".bib") {
            name.push_str(".bib");
        }
        let stem = Path::new(&name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("references");
        let out_name = format!("{}.typst.bib", stem);
        let out_path = out_dir.join(&out_name);
        let src_path = base_dir.join(&name);
        if src_path.exists() {
            let content = fs::read_to_string(&src_path)?;
            let sanitized = sanitize_bibtex_content(&content);
            if entries.len() > 1 {
                merged_entries.extend(extract_bib_entries(&sanitized));
            } else {
                fs::write(&out_path, sanitized)?;
            }
        } else if !out_path.exists() {
            let placeholder = "% Missing bibliography source. Provide a .bib file to populate citations.\n";
            fs::write(&out_path, placeholder)?;
        }
        mapping.insert(name, out_name);
    }

    if entries.len() > 1 && !merged_entries.is_empty() {
        let merged_name = "references.typst.bib".to_string();
        let merged_path = out_dir.join(&merged_name);
        let mut seen = std::collections::HashSet::new();
        let mut merged = String::new();
        for (key, entry_text) in merged_entries {
            let key_norm = key.to_lowercase();
            if seen.insert(key_norm) {
                merged.push_str(&entry_text);
                if !merged.ends_with('\n') {
                    merged.push('\n');
                }
            }
        }
        fs::write(&merged_path, merged)?;
        for value in mapping.values_mut() {
            *value = merged_name.clone();
        }
    }

    Ok(mapping)
}

#[cfg(feature = "cli")]
fn extract_bib_entries(content: &str) -> Vec<(String, String)> {
    let bytes = content.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    let mut anon_idx = 0usize;

    while i < bytes.len() {
        if bytes[i] == b'@' {
            let start = i;
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_alphabetic() {
                j += 1;
            }
            if j == i + 1 {
                i += 1;
                continue;
            }
            let entry_type = &content[i + 1..j];
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j >= bytes.len() || (bytes[j] != b'{' && bytes[j] != b'(') {
                i += 1;
                continue;
            }
            let open = bytes[j] as char;
            let close = if open == '{' { '}' } else { ')' };
            j += 1;

            // Extract key
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            let key_start = j;
            while j < bytes.len() && bytes[j] != b',' && bytes[j] != close as u8 {
                j += 1;
            }
            let key = content[key_start..j].trim().to_string();

            // Scan to matching close
            let mut depth = 1i32;
            while j < bytes.len() && depth > 0 {
                let ch = bytes[j] as char;
                if ch == open {
                    depth += 1;
                } else if ch == close {
                    depth -= 1;
                }
                j += 1;
            }
            let end = j.min(bytes.len());
            let entry_text = content[start..end].to_string();

            let key_final = if entry_type.eq_ignore_ascii_case("string")
                || entry_type.eq_ignore_ascii_case("preamble")
                || entry_type.eq_ignore_ascii_case("comment")
                || key.is_empty()
            {
                anon_idx += 1;
                format!("{}-{}", entry_type, anon_idx)
            } else {
                key
            };
            out.push((key_final, entry_text));
            i = end;
            continue;
        }
        i += 1;
    }

    out
}

#[cfg(feature = "cli")]
fn rewrite_bibliography_paths(
    content: &str,
    mapping: &std::collections::HashMap<String, String>,
) -> String {
    let mut out = content.to_string();
    for (orig, new_name) in mapping {
        let needle = format!("\"{}\"", orig);
        let replacement = format!("\"{}\"", new_name);
        out = out.replace(&needle, &replacement);
    }
    out
}

#[cfg(feature = "cli")]
fn collect_bibliography_entries_with_includes(
    input: &str,
    base_dir: &Path,
) -> Vec<String> {
    let mut entries = collect_bibliography_entries(input);
    if !entries.is_empty() {
        return entries;
    }

    for inc in collect_include_paths(input) {
        let mut candidate = base_dir.join(&inc);
        if candidate.extension().is_none() {
            let with_tex = candidate.with_extension("tex");
            if with_tex.exists() {
                candidate = with_tex;
            }
        }
        if let Ok(content) = fs::read_to_string(&candidate) {
            let nested = collect_bibliography_entries(&content);
            entries.extend(nested);
        }
    }

    if !entries.is_empty() {
        return entries;
    }

    // Fallback: collect .bib files from references/ or bibliography/ folders.
    for folder in ["references", "bibliography"] {
        let dir = base_dir.join(folder);
        if let Ok(read_dir) = fs::read_dir(&dir) {
            for entry in read_dir.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) != Some("bib") {
                    continue;
                }
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    entries.push(format!("{}/{}", folder, stem));
                }
            }
        }
    }

    // Final fallback: collect .bib files from the base directory.
    if let Ok(read_dir) = fs::read_dir(base_dir) {
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("bib") {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                entries.push(stem.to_string());
            }
        }
    }

    entries
}

#[cfg(feature = "cli")]
fn collect_include_paths(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && (input[i..].starts_with("\\input") || input[i..].starts_with("\\include"))
        {
            let cmd_len = if input[i..].starts_with("\\input") {
                "\\input".len()
            } else {
                "\\include".len()
            };
            let after = i + cmd_len;
            if after < bytes.len() && bytes[after].is_ascii_alphabetic() {
                i += 1;
                continue;
            }
            let mut j = after;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'{' {
                let mut depth = 0i32;
                let mut end = None;
                for (off, ch) in input[j..].char_indices() {
                    match ch {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                end = Some(j + off);
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                if let Some(end_pos) = end {
                    let content = input[j + 1..end_pos].trim();
                    if !content.is_empty() {
                        out.push(content.to_string());
                    }
                    i = end_pos + 1;
                    continue;
                }
            }
            i = after;
        }
        i += 1;
    }
    out
}

#[cfg(feature = "cli")]
fn resolve_graphics_path(
    raw: &str,
    graphic_dirs: &[String],
    base_dir: &Path,
) -> Option<(PathBuf, String)> {
    const EXTENSIONS: &[&str] = &[".pdf", ".png", ".jpg", ".jpeg", ".eps", ".svg"];

    if raw.is_empty() || raw.contains('\\') || raw.contains('{') || raw.contains('}') {
        return None;
    }

    let raw_path = Path::new(raw);
    let mut bases: Vec<PathBuf> = Vec::new();
    if raw_path.is_absolute() {
        bases.push(PathBuf::new());
    } else {
        bases.push(base_dir.to_path_buf());
        for dir in graphic_dirs {
            let trimmed = dir.trim();
            if trimmed.is_empty() {
                continue;
            }
            let candidate = PathBuf::from(trimmed);
            if candidate.is_absolute() {
                bases.push(candidate);
            } else {
                bases.push(base_dir.join(candidate));
            }
        }
    }

    let has_ext = raw_path.extension().is_some();
    if has_ext {
        for base in &bases {
            let candidate = if raw_path.is_absolute() {
                raw_path.to_path_buf()
            } else {
                base.join(raw_path)
            };
            if candidate.exists() {
                return Some((candidate, raw.to_string()));
            }
        }
        return None;
    }

    for base in &bases {
        let candidate = if raw_path.is_absolute() {
            raw_path.to_path_buf()
        } else {
            base.join(raw_path)
        };
        if candidate.exists() {
            return Some((candidate, raw.to_string()));
        }
    }

    for ext in EXTENSIONS {
        for base in &bases {
            let mut with_ext = raw.to_string();
            with_ext.push_str(ext);
            let candidate = if raw_path.is_absolute() {
                PathBuf::from(&with_ext)
            } else {
                base.join(&with_ext)
            };
            if candidate.exists() {
                return Some((candidate, with_ext));
            }
        }
    }

    None
}

#[cfg(feature = "cli")]
fn copy_graphics_assets(
    paths: &[String],
    graphic_dirs: &[String],
    base_dir: &Path,
    out_dir: &Path,
) -> io::Result<std::collections::HashMap<String, String>> {
    let mut mapping = std::collections::HashMap::new();
    let mut seen = std::collections::HashSet::new();
    let mut missing: Vec<String> = Vec::new();

    for raw in paths {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some((src, rel)) = resolve_graphics_path(trimmed, graphic_dirs, base_dir) {
            if seen.insert(rel.clone()) {
                let dest = out_dir.join(&rel);
                if dest == src {
                    continue;
                }
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut copied = fs::copy(&src, &dest).is_ok();
                if copied {
                    if let (Ok(src_meta), Ok(dest_meta)) = (fs::metadata(&src), fs::metadata(&dest)) {
                        if src_meta.len() > 0 && dest_meta.len() == 0 {
                            copied = false;
                        }
                    }
                }
                if !copied {
                    if let Ok(data) = fs::read(&src) {
                        fs::write(&dest, data)?;
                        copied = true;
                    }
                }
                if !copied {
                    missing.push(trimmed.to_string());
                }
            }
            if trimmed != rel {
                mapping.insert(trimmed.to_string(), rel);
            }
        } else {
            missing.push(trimmed.to_string());
        }
    }

    if !missing.is_empty() {
        let placeholder_name = "missing-image.svg";
        let placeholder_path = out_dir.join(placeholder_name);
        if !placeholder_path.exists() {
            let svg = r##"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="640" height="360" viewBox="0 0 640 360">
  <rect x="0" y="0" width="640" height="360" fill="#fff5f5" stroke="#cc0000" stroke-width="4"/>
  <line x1="0" y1="0" x2="640" y2="360" stroke="#cc0000" stroke-width="3"/>
  <line x1="640" y1="0" x2="0" y2="360" stroke="#cc0000" stroke-width="3"/>
  <text x="320" y="190" text-anchor="middle" font-family="Helvetica, Arial, sans-serif" font-size="24" fill="#cc0000">Missing image</text>
</svg>
"##;
            fs::write(&placeholder_path, svg)?;
        }
        for raw in missing {
            mapping.insert(raw, placeholder_name.to_string());
        }
    }

    Ok(mapping)
}

#[cfg(feature = "cli")]
fn rewrite_image_paths(
    content: &str,
    mapping: &std::collections::HashMap<String, String>,
) -> String {
    let mut out = content.to_string();
    for (orig, new_name) in mapping {
        let needle = format!("\"{}\"", orig);
        let replacement = format!("\"{}\"", new_name);
        out = out.replace(&needle, &replacement);
    }
    out
}

#[cfg(feature = "cli")]
fn rewrite_extensionless_images(content: &str, out_dir: &Path) -> String {
    const EXTENSIONS: &[&str] = &[".pdf", ".png", ".jpg", ".jpeg", ".eps", ".svg"];

    let mut out = String::with_capacity(content.len());
    let mut i = 0usize;
    while let Some(pos) = content[i..].find("image(\"") {
        let start = i + pos;
        if start > 0 {
            let prev = content.as_bytes()[start - 1] as char;
            if prev.is_ascii_alphanumeric() || prev == '_' {
                out.push_str(&content[i..start + 1]);
                i = start + 1;
                continue;
            }
        }

        out.push_str(&content[i..start]);
        out.push_str("image(\"");

        let path_start = start + "image(\"".len();
        let Some(end_rel) = content[path_start..].find('"') else {
            out.push_str(&content[path_start..]);
            return out;
        };
        let path_end = path_start + end_rel;
        let path = &content[path_start..path_end];

        let mut replacement: Option<String> = None;
        if Path::new(path).extension().is_none() {
            for ext in EXTENSIONS {
                let mut with_ext = path.to_string();
                with_ext.push_str(ext);
                let candidate = if Path::new(path).is_absolute() {
                    PathBuf::from(&with_ext)
                } else {
                    out_dir.join(&with_ext)
                };
                if candidate.exists() {
                    replacement = Some(with_ext);
                    break;
                }
            }
        }

        if let Some(new_path) = replacement {
            out.push_str(&new_path);
        } else {
            out.push_str(path);
        }
        out.push('"');
        i = path_end + 1;
    }

    out.push_str(&content[i..]);
    out
}

#[cfg(feature = "cli")]
fn handle_subcommand(cmd: Commands) -> io::Result<()> {
    match cmd {
        Commands::Check { input, no_color } => {
            let content = match input {
                Some(path) => fs::read_to_string(&path)?,
                None => {
                    let mut buffer = String::new();
                    io::stdin().read_to_string(&mut buffer)?;
                    buffer
                }
            };

            let result = check_latex(&content);
            let output = format_diagnostics(&result, !no_color);
            println!("{}", output);

            if result.has_errors() {
                std::process::exit(1);
            }
        }

        Commands::Convert {
            input,
            output,
            direction,
            full_document,
            ir,
            auto_repair,
            ai_cmd,
            loss_log,
            post_repair_log,
            allow_no_gain,
        } => {
            let (mut content, filename) = match input {
                Some(ref path) => (fs::read_to_string(path)?, Some(path.clone())),
                None => {
                    let mut buffer = String::new();
                    io::stdin().read_to_string(&mut buffer)?;
                    (buffer, None)
                }
            };

            let direction = match direction {
                Direction::Auto => {
                    if let Some(ref name) = filename {
                        if name.ends_with(".typ") {
                            Direction::T2l
                        } else if name.ends_with(".tex") {
                            Direction::L2t
                        } else {
                            let format = detect_format(&content);
                            if format == "latex" {
                                Direction::L2t
                            } else {
                                Direction::T2l
                            }
                        }
                    } else {
                        let format = detect_format(&content);
                        if format == "latex" {
                            Direction::L2t
                        } else {
                            Direction::T2l
                        }
                    }
                }
                d => d,
            };

            let mut bib_entries: Vec<String> = Vec::new();
            let mut bib_base_dir: Option<std::path::PathBuf> = None;
            let mut graphic_paths: Vec<String> = Vec::new();
            let mut graphic_dirs: Vec<String> = Vec::new();
            let mut graphics_base_dir: Option<PathBuf> = None;

            if matches!(direction, Direction::L2t) && full_document {
                if let Some(path) = filename.as_ref() {
                    if let Some(parent) = Path::new(path).parent() {
                        content = expand_latex_inputs(&content, parent);
                        bib_entries = collect_bibliography_entries(&content);
                        bib_base_dir = Some(parent.to_path_buf());
                        graphic_dirs = collect_graphicspath_entries(&content);
                        graphic_paths = collect_includegraphics_paths(&content);
                        graphics_base_dir = Some(parent.to_path_buf());
                    }
                }
            }

            let repair_config = AiRepairConfig {
                auto_repair,
                ai_cmd: ai_cmd.clone(),
                allow_no_gain,
            };
            let mut loss_report: Option<LossReport> = None;
            let mut post_report: Option<LossReport> = None;

            let mut result = if full_document {
                match direction {
                    Direction::L2t => {
                        if auto_repair || loss_log.is_some() || post_repair_log.is_some() {
                            let report = latex_to_typst_with_report(&content);
                            loss_report = Some(report.report.clone());
                            let repaired = tylax::utils::repair::maybe_repair_latex_to_typst(
                                &content,
                                &report.content,
                                &report.report,
                                &repair_config,
                            );
                            if post_repair_log.is_some() {
                                post_report = Some(build_post_report_typst(&repaired));
                            }
                            repaired
                        } else {
                            latex_document_to_typst(&content)
                        }
                    }
                    Direction::T2l => {
                        let use_ir = ir || auto_repair || loss_log.is_some() || post_repair_log.is_some();
                        if auto_repair || loss_log.is_some() || post_repair_log.is_some() {
                            let report = typst_to_latex_ir_with_report(&content, true);
                            loss_report = Some(report.report.clone());
                            let repaired = maybe_repair_typst_to_latex(
                                &content,
                                &report.content,
                                &report.report,
                                &repair_config,
                            );
                            if post_repair_log.is_some() {
                                post_report = Some(build_post_report_latex(&repaired));
                            }
                            repaired
                        } else if use_ir {
                            typst_to_latex_ir(&content, true)
                        } else {
                            typst_document_to_latex(&content)
                        }
                    }
                    Direction::Auto => convert_auto_document(&content).0,
                }
            } else {
                match direction {
                    Direction::L2t => {
                        if auto_repair || loss_log.is_some() || post_repair_log.is_some() {
                            let report = latex_math_to_typst_with_report(&content);
                            loss_report = Some(report.report.clone());
                            let repaired = tylax::utils::repair::maybe_repair_latex_to_typst(
                                &content,
                                &report.content,
                                &report.report,
                                &repair_config,
                            );
                            if post_repair_log.is_some() {
                                post_report = Some(build_post_report_typst(&repaired));
                            }
                            repaired
                        } else {
                            latex_to_typst(&content)
                        }
                    }
                    Direction::T2l => {
                        let use_ir = ir || auto_repair || loss_log.is_some() || post_repair_log.is_some();
                        if auto_repair || loss_log.is_some() || post_repair_log.is_some() {
                            let report = typst_to_latex_ir_with_report(&content, false);
                            loss_report = Some(report.report.clone());
                            let repaired = maybe_repair_typst_to_latex(
                                &content,
                                &report.content,
                                &report.report,
                                &repair_config,
                            );
                            if post_repair_log.is_some() {
                                post_report = Some(build_post_report_latex(&repaired));
                            }
                            repaired
                        } else if use_ir {
                            typst_to_latex_ir(&content, false)
                        } else {
                            typst_to_latex(&content)
                        }
                    }
                    Direction::Auto => convert_auto(&content).0,
                }
            };

            if !bib_entries.is_empty() {
                if let Some(output_path) = output.as_ref() {
                    let out_dir = Path::new(output_path)
                        .parent()
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(|| std::path::PathBuf::from("."));
                    if let Some(base_dir) = bib_base_dir.as_ref() {
                        if let Ok(mapping) = sanitize_bibliography_files(&bib_entries, base_dir, &out_dir) {
                            result = rewrite_bibliography_paths(&result, &mapping);
                        }
                    }
                }
            }
            if let (Some(output_path), Some(base_dir)) = (output.as_ref(), graphics_base_dir.as_ref()) {
                let out_dir = Path::new(output_path)
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| std::path::PathBuf::from("."));
                if !graphic_paths.is_empty() {
                    if let Ok(mapping) =
                        copy_graphics_assets(&graphic_paths, &graphic_dirs, base_dir, &out_dir)
                    {
                        result = rewrite_image_paths(&result, &mapping);
                    }
                }
                result = rewrite_extensionless_images(&result, &out_dir);
            }

            if let (Some(path), Some(report)) = (loss_log.as_ref(), loss_report.as_ref()) {
                let serialized = serde_json::to_string_pretty(report)
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
                fs::write(path, serialized)?;
            }
            if let (Some(path), Some(report)) = (post_repair_log.as_ref(), post_report.as_ref()) {
                let serialized = serde_json::to_string_pretty(report)
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
                fs::write(path, serialized)?;
            }

            match output {
                Some(path) => {
                    let mut file = fs::File::create(&path)?;
                    writeln!(file, "{}", result)?;
                    eprintln!("✓ Output written to: {}", path);
                }
                None => {
                    println!("{}", result);
                }
            }
        }

        Commands::Tikz {
            input,
            output,
            direction,
        } => {
            let content = match input {
                Some(path) => fs::read_to_string(&path)?,
                None => {
                    let mut buffer = String::new();
                    io::stdin().read_to_string(&mut buffer)?;
                    buffer
                }
            };

            let direction = match direction {
                TikzDirection::Auto => {
                    if is_cetz_code(&content) {
                        TikzDirection::CetzToTikz
                    } else {
                        TikzDirection::TikzToCetz
                    }
                }
                d => d,
            };

            let result = match direction {
                TikzDirection::TikzToCetz => convert_tikz_to_cetz(&content),
                TikzDirection::CetzToTikz => convert_cetz_to_tikz(&content),
                TikzDirection::Auto => unreachable!(),
            };

            match output {
                Some(path) => {
                    let mut file = fs::File::create(&path)?;
                    writeln!(file, "{}", result)?;
                    eprintln!("✓ TikZ/CeTZ conversion written to: {}", path);
                }
                None => {
                    println!("{}", result);
                }
            }
        }

        Commands::Batch {
            input,
            output_dir,
            direction,
            full_document,
            extension,
            ir,
        } => {
            // Create output directory if it doesn't exist
            fs::create_dir_all(&output_dir)?;

            // Determine output extension
            let out_ext = extension.unwrap_or_else(|| match direction {
                Direction::L2t => "typ".to_string(),
                Direction::T2l => "tex".to_string(),
                Direction::Auto => "out".to_string(),
            });

            // Find input files
            let input_path = Path::new(&input);
            let files: Vec<_> = if input_path.is_dir() {
                // Read all .tex or .typ files from directory
                fs::read_dir(input_path)?
                    .filter_map(|e| e.ok())
                    .filter(|e| {
                        let path = e.path();
                        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
                        matches!(direction, Direction::L2t) && ext == "tex"
                            || matches!(direction, Direction::T2l) && ext == "typ"
                            || matches!(direction, Direction::Auto)
                    })
                    .map(|e| e.path())
                    .collect()
            } else {
                // Single file
                vec![input_path.to_path_buf()]
            };

            let mut success_count = 0;
            let mut error_count = 0;

            for file_path in files {
                let filename = file_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("output");

                let output_path = Path::new(&output_dir).join(format!("{}.{}", filename, out_ext));

                match fs::read_to_string(&file_path) {
                    Ok(content) => {
                        let result = if full_document {
                            match direction {
                                Direction::L2t => latex_document_to_typst(&content),
                                Direction::T2l => {
                                    if ir {
                                        typst_to_latex_ir(&content, true)
                                    } else {
                                        typst_document_to_latex(&content)
                                    }
                                }
                                Direction::Auto => convert_auto_document(&content).0,
                            }
                        } else {
                            match direction {
                                Direction::L2t => latex_to_typst(&content),
                                Direction::T2l => {
                                    if ir {
                                        typst_to_latex_ir(&content, false)
                                    } else {
                                        typst_to_latex(&content)
                                    }
                                }
                                Direction::Auto => convert_auto(&content).0,
                            }
                        };

                        match fs::write(&output_path, &result) {
                            Ok(_) => {
                                eprintln!("✓ {}", output_path.display());
                                success_count += 1;
                            }
                            Err(e) => {
                                eprintln!("✗ {} - write error: {}", output_path.display(), e);
                                error_count += 1;
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("✗ {} - read error: {}", file_path.display(), e);
                        error_count += 1;
                    }
                }
            }

            eprintln!(
                "\nBatch conversion complete: {} succeeded, {} failed",
                success_count, error_count
            );

            if error_count > 0 {
                std::process::exit(1);
            }
        }

        Commands::Info => {
            println!("Tylax - High-performance bidirectional LaTeX ↔ Typst converter");
            println!("Version: {}", env!("CARGO_PKG_VERSION"));
            println!();
            println!("Features:");
            println!("  ✓ LaTeX → Typst conversion (math + documents)");
            println!("  ✓ Typst → LaTeX conversion (math + documents)");
            println!("  ✓ TikZ ↔ CeTZ graphics conversion");
            println!("  ✓ Batch file processing");
            println!("  ✓ LaTeX diagnostics and checking");
            println!("  ✓ Auto-detection of input format");
            println!();
            println!("Supported packages:");
            println!("  - amsmath, amssymb, mathtools");
            println!("  - graphicx, hyperref, biblatex");
            println!("  - tikz, pgf (basic features)");
            println!("  - siunitx, mhchem");
            println!();
            println!("Repository: https://github.com/scipenai/tylax");
            println!();
        }
    }

    Ok(())
}

#[cfg(feature = "cli")]
fn pretty_print(input: &str) -> String {
    // Simple pretty printing: normalize indentation and spacing
    let mut result = String::new();
    let mut indent_level: usize = 0;

    for line in input.lines() {
        let trimmed = line.trim();

        // Decrease indent before closing braces/brackets
        if trimmed.starts_with('}') || trimmed.starts_with(']') || trimmed.starts_with(')') {
            indent_level = indent_level.saturating_sub(1);
        }
        if trimmed.starts_with("\\end{") {
            indent_level = indent_level.saturating_sub(1);
        }

        // Add indentation
        for _ in 0..indent_level {
            result.push_str("  ");
        }
        result.push_str(trimmed);
        result.push('\n');

        // Increase indent after opening braces/brackets
        if trimmed.ends_with('{') || trimmed.ends_with('[') {
            indent_level += 1;
        }
        if trimmed.starts_with("\\begin{") {
            indent_level += 1;
        }
    }

    result.trim().to_string()
}

fn build_post_report_typst(output: &str) -> LossReport {
    let metrics = typst_metrics_source(output, LOSS_MARKER_PREFIX);
    let mut records = Vec::new();
    for idx in 0..metrics.loss_markers {
        let id = format!("L{:04}", idx + 1);
        records.push(LossRecord::new(
            id,
            LossKind::Other,
            Some("post-repair-loss-marker".to_string()),
            "loss marker present after repair",
            None,
            None,
        ));
    }
    LossReport::new("latex", "typst", records, Vec::new())
}

fn build_post_report_latex(output: &str) -> LossReport {
    let metrics = latex_metrics_source(output, LOSS_MARKER_PREFIX);
    let mut records = Vec::new();
    for idx in 0..metrics.loss_markers {
        let id = format!("L{:04}", idx + 1);
        records.push(LossRecord::new(
            id,
            LossKind::Other,
            Some("post-repair-loss-marker".to_string()),
            "loss marker present after repair",
            None,
            None,
        ));
    }
    LossReport::new("typst", "latex", records, Vec::new())
}

#[cfg(not(feature = "cli"))]
fn main() {
    eprintln!("CLI feature not enabled. Build with --features cli");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("  cargo install tylax --features cli");
    eprintln!("  t2l [OPTIONS] [INPUT_FILE]");
}
