//! Tylax CLI - High-performance bidirectional LaTeX ↔ Typst converter

#[cfg(feature = "cli")]
use clap::{Parser, Subcommand, ValueEnum};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Instant;
use tylax::core::latex2typst::utils::{
    collect_bibliography_entries, collect_graphicspath_entries, collect_includegraphics_paths,
    collect_usepackage_entries, expand_latex_inputs, expand_local_packages_with_skip,
    sanitize_bibtex_content, sanitize_citation_key,
};
use tylax::{
    convert_auto, convert_auto_document, detect_format,
    diagnostics::{check_latex, format_diagnostics},
    latex_document_to_typst, latex_math_to_typst_with_report, latex_to_typst,
    latex_to_typst_with_diagnostics, latex_to_typst_with_report,
    tikz::{convert_cetz_to_tikz, convert_tikz_to_cetz, is_cetz_code},
    typst_document_to_latex, typst_to_latex, typst_to_latex_ir, typst_to_latex_ir_with_report,
    typst_to_latex_with_diagnostics,
    utils::latex_analysis::metrics_source as latex_metrics_source,
    utils::loss::{LossKind, LossRecord, LossReport, LOSS_MARKER_PREFIX},
    utils::repair::{maybe_repair_typst_to_latex, AiRepairConfig},
    utils::typst_analysis::metrics_source as typst_metrics_source,
    CliDiagnostic, T2LOptions,
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

    /// Disable MiniEval preprocessing for Typst scripting features (loops, functions)
    /// By default, MiniEval is enabled for T2L conversions to expand #let, #for, #if, etc.
    #[arg(long)]
    no_eval: bool,

    /// Strict mode: exit with error if any conversion warnings occur
    #[arg(long)]
    strict: bool,

    /// Quiet mode: suppress warning output to stderr
    #[arg(short, long)]
    quiet: bool,

    /// Embed warnings as comments in the output file
    #[arg(long)]
    embed_warnings: bool,
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
    // Spawn main logic in a thread with larger stack to handle deeply nested documents
    // mitex parser can create deeply nested ASTs for complex LaTeX templates
    const STACK_SIZE: usize = 256 * 1024 * 1024; // 256 MB stack
    let builder = std::thread::Builder::new().stack_size(STACK_SIZE);
    let handle = builder.spawn(main_inner).expect("Failed to spawn main thread");
    handle.join().expect("Main thread panicked")
}

#[cfg(feature = "cli")]
fn main_inner() -> io::Result<()> {
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

    // Determine if this is a full document based on content or flag.
    let is_full_document = cli.full_document || is_latex_document(&input);
    let template_assets = if matches!(direction, Direction::L2t) && is_full_document {
        let packages = collect_usepackage_entries(&input);
        TemplateAssetFlags {
            cvpr: packages
                .iter()
                .any(|pkg| pkg.trim().to_lowercase().starts_with("cvpr")),
            iclr: packages
                .iter()
                .any(|pkg| pkg.trim().to_lowercase().starts_with("iclr")),
            icml: packages
                .iter()
                .any(|pkg| pkg.trim().to_lowercase().starts_with("icml")),
            neurips: packages
                .iter()
                .any(|pkg| pkg.trim().to_lowercase().starts_with("neurips")),
            jmlr: packages
                .iter()
                .any(|pkg| pkg.trim().to_lowercase().starts_with("jmlr")),
            tmlr: packages
                .iter()
                .any(|pkg| pkg.trim().to_lowercase().starts_with("tmlr")),
            rlj: packages.iter().any(|pkg| {
                let lower = pkg.trim().to_lowercase();
                lower.starts_with("rlj") || lower.starts_with("rlc")
            }),
        }
    } else {
        TemplateAssetFlags::default()
    };

    let timing_enabled = std::env::var("TYLAX_TIMING").is_ok();
    if matches!(direction, Direction::L2t) && is_full_document {
        if let Some(path) = filename.as_ref() {
            if let Some(parent) = Path::new(path).parent() {
                let start_expand = Instant::now();
                if timing_enabled {
                    eprintln!("[tylax] expand inputs: start");
                }
                input = expand_latex_inputs(&input, parent);
                let mut skip_packages = std::collections::HashSet::new();
                let mut skipped_list: Vec<String> = Vec::new();
                for pkg in collect_usepackage_entries(&input) {
                    if is_macro_expansion_blacklisted_package(&pkg) {
                        let lower = pkg.trim().to_lowercase();
                        if skip_packages.insert(lower.clone()) {
                            skipped_list.push(lower);
                        }
                    }
                }
                if !skipped_list.is_empty() {
                    skipped_list.sort();
                    skipped_list.dedup();
                    eprintln!(
                        "⚠ Skipping local package expansion for: {}",
                        skipped_list.join(", ")
                    );
                }
                input = expand_local_packages_with_skip(&input, parent, &skip_packages);
                if timing_enabled {
                    let secs = start_expand.elapsed().as_secs_f64();
                    eprintln!("[tylax] expand inputs: {:.3}s", secs);
                }
                bib_entries = collect_bibliography_entries(&input);
                if bib_entries.is_empty() {
                    bib_entries = collect_bibliography_entries_with_includes(&input, parent);
                }
                bib_base_dir = Some(parent.to_path_buf());
                graphic_dirs = collect_graphicspath_entries(&input);
                graphic_paths = collect_includegraphics_paths(&input);
                graphics_base_dir = Some(parent.to_path_buf());
                input = inject_class_hints(&input, parent)?;
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

    let mut diagnostics: Vec<CliDiagnostic> = Vec::new();
    // Convert
    let mut result = match direction {
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
                let conv_result = latex_to_typst_with_diagnostics(&input);
                diagnostics = conv_result
                    .warnings
                    .into_iter()
                    .map(CliDiagnostic::from)
                    .collect();
                conv_result.output
            }
        }
        Direction::T2l => {
            let options = if is_full_document {
                T2LOptions::full_document()
            } else {
                T2LOptions::default()
            };
            let use_ir = cli.ir
                || cli.auto_repair
                || cli.loss_log.is_some()
                || cli.post_repair_log.is_some();
            if cli.auto_repair || cli.loss_log.is_some() || cli.post_repair_log.is_some() {
                let report = typst_to_latex_ir_with_report(&input, is_full_document);
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
                typst_to_latex_ir(&input, is_full_document)
            } else if !cli.no_eval {
                let conv_result = typst_to_latex_with_diagnostics(&input, &options);
                diagnostics = conv_result
                    .warnings
                    .into_iter()
                    .map(CliDiagnostic::from)
                    .collect();
                conv_result.output
            } else {
                if is_full_document {
                    typst_document_to_latex(&input)
                } else {
                    typst_to_latex(&input)
                }
            }
        }
        Direction::Auto => {
            if is_full_document {
                convert_auto_document(&input).0
            } else {
                convert_auto(&input).0
            }
        }
    };

    // Print diagnostics to stderr (unless quiet mode)
    if !cli.quiet && !diagnostics.is_empty() {
        print_diagnostics_to_stderr(&diagnostics, cli.color);
    }

    // Check strict mode
    if cli.strict && !diagnostics.is_empty() {
        eprintln!(
            "Error: {} conversion warning(s) in strict mode",
            diagnostics.len()
        );
        std::process::exit(1);
    }

    // Embed diagnostics as comments if requested
    if cli.embed_warnings && !diagnostics.is_empty() {
        result = embed_diagnostics_as_comments(&result, &diagnostics);
    }

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

    // Bibliography handling
    if let Some(output_path) = cli.output.as_ref() {
        let out_dir = Path::new(output_path)
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let mut citation_keys = extract_citation_keys_from_typst(&result);
        let label_keys = extract_typst_labels(&result);
        citation_keys.retain(|key| !label_keys.contains(key));
        let bibitem_keys = extract_bibitem_keys_from_typst(&result);
        citation_keys.extend(bibitem_keys.iter().cloned());

        if !bib_entries.is_empty() {
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
                    let mut mapped_files: Vec<String> = mapping.values().cloned().collect();
                    mapped_files.sort();
                    mapped_files.dedup();
                    let _ =
                        populate_placeholder_bibliography(&out_dir, &mapped_files, &citation_keys);
                }
            }
        } else if !bibitem_keys.is_empty() && !result.contains("#bibliography") {
            let stub_path = out_dir.join("references.typst.bib");
            let _ = write_stub_bibliography(&stub_path, &citation_keys);
            result.push_str("\n#hide(bibliography(\"references.typst.bib\"))\n");
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
            if template_assets.any() {
                let out_dir = Path::new(&path)
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| PathBuf::from("."));
                if let Err(err) = write_ml_template_assets(&out_dir, &template_assets) {
                    eprintln!("⚠ Unable to write Typst template assets: {}", err);
                }
            }
            let mut file = fs::File::create(&path)?;
            writeln!(file, "{}", result)?;
            if diagnostics.is_empty() {
                eprintln!("✓ Output written to: {}", path);
            } else {
                eprintln!(
                    "⚠ Output written to: {} ({} warning(s))",
                    path,
                    diagnostics.len()
                );
            }
        }
        None => {
            println!("{}", result);
        }
    }

    Ok(())
}

#[derive(Default, Clone, Copy)]
struct TemplateAssetFlags {
    cvpr: bool,
    iclr: bool,
    icml: bool,
    neurips: bool,
    jmlr: bool,
    tmlr: bool,
    rlj: bool,
}

impl TemplateAssetFlags {
    fn any(&self) -> bool {
        self.cvpr
            || self.iclr
            || self.icml
            || self.neurips
            || self.jmlr
            || self.tmlr
            || self.rlj
    }
}

#[cfg(feature = "cli")]
fn is_macro_expansion_blacklisted_package(name: &str) -> bool {
    let lower = name.trim().to_lowercase();
    lower == "eccv"
        || lower.starts_with("iclr")
        || lower.starts_with("icml")
        || lower.starts_with("mlsys")
        || lower.starts_with("neurips")
        || lower.starts_with("nips") // older NeurIPS style files (nips_2018, etc.)
        || lower.starts_with("aaai")
        || lower.starts_with("cvpr")
        || lower.starts_with("jmlr")
        || lower.starts_with("tmlr")
        || lower.starts_with("rlj")
        || lower.starts_with("rlc")
        || lower.starts_with("colm") // COLM conference
        || lower == "natbib" // complex citation macros with many \expandafter
        || lower == "fancyhdr" // header/footer macros
        || lower.starts_with("bxcoloremoji")
        || lower == "cjkutf8"
}

fn extract_braced_arg_at(input: &str, start: usize) -> Option<(String, usize)> {
    let bytes = input.as_bytes();
    let mut i = start;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }
    if bytes[i] != b'{' {
        if let Some(pos) = input[i..].find('{') {
            i += pos;
        } else {
            return None;
        }
    }
    let mut depth = 0i32;
    let mut j = i;
    while j < bytes.len() {
        match bytes[j] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    let content = input[i + 1..j].to_string();
                    return Some((content, j + 1));
                }
            }
            _ => {}
        }
        j += 1;
    }
    None
}

fn extract_bracket_arg_at(input: &str, start: usize) -> (Option<String>, usize) {
    let bytes = input.as_bytes();
    let mut i = start;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if i >= bytes.len() {
        return (None, i);
    }
    if bytes[i] != b'[' {
        return (None, i);
    }
    let mut depth = 0i32;
    let mut j = i;
    while j < bytes.len() {
        match bytes[j] {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    let content = input[i + 1..j].to_string();
                    return (Some(content), j + 1);
                }
            }
            _ => {}
        }
        j += 1;
    }
    (None, j)
}

fn extract_documentclass_name(input: &str) -> Option<String> {
    let mut pos = 0usize;
    while let Some(idx) = input[pos..].find("\\documentclass") {
        let start = pos + idx + "\\documentclass".len();
        let (opt, next) = extract_bracket_arg_at(input, start);
        let cursor = if let Some(_) = opt { next } else { start };
        if let Some((arg, _)) = extract_braced_arg_at(input, cursor) {
            let name = arg.trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
        pos = start + 1;
    }
    None
}

fn inject_class_hints(input: &str, base_dir: &Path) -> io::Result<String> {
    if input.contains("__TYLAX_HINTS_BEGIN__") {
        return Ok(input.to_string());
    }
    let class_name = match extract_documentclass_name(input) {
        Some(name) => name,
        None => return Ok(input.to_string()),
    };
    let class_path = base_dir.join(format!("{class_name}.cls"));
    if !class_path.exists() {
        return Ok(input.to_string());
    }
    let metadata = class_path.metadata()?;
    if metadata.len() > 1_000_000 {
        return Ok(input.to_string());
    }
    let content = std::fs::read_to_string(&class_path)?;
    let mut hint_block = String::new();
    hint_block.push_str("\n%__TYLAX_HINTS_BEGIN__\n");
    for line in content.lines() {
        hint_block.push('%');
        hint_block.push_str(line);
        hint_block.push('\n');
    }
    hint_block.push_str("%__TYLAX_HINTS_END__\n");
    if let Some(pos) = input.find("\\begin{document}") {
        let mut out = String::with_capacity(input.len() + hint_block.len());
        out.push_str(&input[..pos]);
        out.push_str(&hint_block);
        out.push_str(&input[pos..]);
        Ok(out)
    } else {
        Ok(format!("{input}{hint_block}"))
    }
}

// Embedded template assets - only available with `embedded-templates` feature
// These require local typst-corpus directory and are not included in remote builds

#[cfg(all(feature = "cli", feature = "embedded-templates"))]
fn write_cvpr_assets(out_dir: &Path) -> io::Result<()> {
    const CVPR: &str = include_str!("../../typst-corpus/ml-templates/cvpr/cvpr.typ");
    const CVPR_2022: &str = include_str!("../../typst-corpus/ml-templates/cvpr/cvpr2022.typ");
    const CVPR_2025: &str = include_str!("../../typst-corpus/ml-templates/cvpr/cvpr2025.typ");
    const LOGO: &str = include_str!("../../typst-corpus/ml-templates/cvpr/logo.typ");

    fs::write(out_dir.join("cvpr.typ"), CVPR)?;
    fs::write(out_dir.join("cvpr2022.typ"), CVPR_2022)?;
    fs::write(out_dir.join("cvpr2025.typ"), CVPR_2025)?;
    fs::write(out_dir.join("logo.typ"), LOGO)?;
    Ok(())
}

#[cfg(all(feature = "cli", feature = "embedded-templates"))]
fn write_iclr_assets(out_dir: &Path) -> io::Result<()> {
    const ICLR: &str = include_str!("../../typst-corpus/ml-templates/iclr/iclr.typ");
    const ICLR_2025: &str =
        include_str!("../../typst-corpus/ml-templates/iclr/iclr2025.typ");
    const ICLR_CSL: &str = include_str!("../../typst-corpus/ml-templates/iclr/iclr.csl");

    fs::write(out_dir.join("iclr.typ"), ICLR)?;
    fs::write(out_dir.join("iclr2025.typ"), ICLR_2025)?;
    fs::write(out_dir.join("iclr.csl"), ICLR_CSL)?;
    Ok(())
}

#[cfg(all(feature = "cli", feature = "embedded-templates"))]
fn write_icml_assets(out_dir: &Path) -> io::Result<()> {
    const ICML: &str = include_str!("../../typst-corpus/ml-templates/icml/icml.typ");
    const ICML_2024: &str =
        include_str!("../../typst-corpus/ml-templates/icml/icml2024.typ");
    const ICML_2025: &str =
        include_str!("../../typst-corpus/ml-templates/icml/icml2025.typ");
    const ICML_CSL: &str = include_str!("../../typst-corpus/ml-templates/icml/icml.csl");

    fs::write(out_dir.join("icml.typ"), ICML)?;
    fs::write(out_dir.join("icml2024.typ"), ICML_2024)?;
    fs::write(out_dir.join("icml2025.typ"), ICML_2025)?;
    fs::write(out_dir.join("icml.csl"), ICML_CSL)?;
    Ok(())
}

#[cfg(all(feature = "cli", feature = "embedded-templates"))]
fn write_neurips_assets(out_dir: &Path) -> io::Result<()> {
    const NEURIPS: &str =
        include_str!("../../typst-corpus/ml-templates/neurips/neurips.typ");
    const NEURIPS_2023: &str =
        include_str!("../../typst-corpus/ml-templates/neurips/neurips2023.typ");
    const NEURIPS_2024: &str =
        include_str!("../../typst-corpus/ml-templates/neurips/neurips2024.typ");
    const NEURIPS_2025: &str =
        include_str!("../../typst-corpus/ml-templates/neurips/neurips2025.typ");
    const NATBIB_CSL: &str =
        include_str!("../../typst-corpus/ml-templates/neurips/natbib.csl");

    fs::write(out_dir.join("neurips.typ"), NEURIPS)?;
    fs::write(out_dir.join("neurips2023.typ"), NEURIPS_2023)?;
    fs::write(out_dir.join("neurips2024.typ"), NEURIPS_2024)?;
    fs::write(out_dir.join("neurips2025.typ"), NEURIPS_2025)?;
    fs::write(out_dir.join("natbib.csl"), NATBIB_CSL)?;
    Ok(())
}

#[cfg(all(feature = "cli", feature = "embedded-templates"))]
fn write_jmlr_assets(out_dir: &Path) -> io::Result<()> {
    const JMLR: &str = include_str!("../../typst-corpus/ml-templates/jmlr/jmlr.typ");
    fs::write(out_dir.join("jmlr.typ"), JMLR)?;
    Ok(())
}

#[cfg(all(feature = "cli", feature = "embedded-templates"))]
fn write_tmlr_assets(out_dir: &Path) -> io::Result<()> {
    const TMLR: &str = include_str!("../../typst-corpus/ml-templates/tmlr/tmlr.typ");
    const TMLR_CSL: &str = include_str!("../../typst-corpus/ml-templates/tmlr/tmlr.csl");
    fs::write(out_dir.join("tmlr.typ"), TMLR)?;
    fs::write(out_dir.join("tmlr.csl"), TMLR_CSL)?;
    Ok(())
}

#[cfg(all(feature = "cli", feature = "embedded-templates"))]
fn write_rlj_assets(out_dir: &Path) -> io::Result<()> {
    const RLJ: &str = include_str!("../../typst-corpus/ml-templates/rlj/rlj.typ");
    fs::write(out_dir.join("rlj.typ"), RLJ)?;
    Ok(())
}

#[cfg(all(feature = "cli", feature = "embedded-templates"))]
fn write_ml_template_assets(out_dir: &Path, flags: &TemplateAssetFlags) -> io::Result<()> {
    if flags.cvpr {
        write_cvpr_assets(out_dir)?;
    }
    if flags.iclr {
        write_iclr_assets(out_dir)?;
    }
    if flags.icml {
        write_icml_assets(out_dir)?;
    }
    if flags.neurips {
        write_neurips_assets(out_dir)?;
    }
    if flags.jmlr {
        write_jmlr_assets(out_dir)?;
    }
    if flags.tmlr {
        write_tmlr_assets(out_dir)?;
    }
    if flags.rlj {
        write_rlj_assets(out_dir)?;
    }
    Ok(())
}

// No-op version when embedded-templates feature is not enabled
#[cfg(all(feature = "cli", not(feature = "embedded-templates")))]
fn write_ml_template_assets(_out_dir: &Path, flags: &TemplateAssetFlags) -> io::Result<()> {
    if flags.any() {
        eprintln!("Note: ML template assets not available (built without embedded-templates feature)");
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
        } else {
            let placeholder = "% TYLAX-STUB-BIB: Missing bibliography source. Provide a .bib file to populate citations.\n";
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
fn extract_citation_keys_from_typst(input: &str) -> std::collections::HashSet<String> {
    let mut keys = std::collections::HashSet::new();
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0usize;
    while i < len {
        // Capture citation blocks like [@key; @key2]
        if chars[i] == '[' {
            let mut probe = i + 1;
            while probe < len && chars[probe].is_whitespace() {
                probe += 1;
            }
            if probe < len && chars[probe] == '@' {
                let mut j = probe + 1;
                while j < len && chars[j] != ']' {
                    j += 1;
                }
                if j < len {
                    let mut k = probe;
                    while k < j {
                        if chars[k] == '@' {
                            if k > 0 && chars[k - 1] == '\\' {
                                k += 1;
                                continue;
                            }
                            let mut m = k + 1;
                            while m < j {
                                let ch = chars[m];
                                if ch.is_ascii_alphanumeric() || ch == '-' {
                                    m += 1;
                                } else {
                                    break;
                                }
                            }
                            if m > k + 1 {
                                let key: String = chars[k + 1..m].iter().collect();
                                keys.insert(key);
                            }
                            k = m;
                            continue;
                        }
                        k += 1;
                    }
                    i = j + 1;
                    continue;
                }
            }
        }

        // Capture #cite(<key>, ...)
        if chars[i] == '#' {
            let target: [char; 6] = ['c', 'i', 't', 'e', '(', '<'];
            if i + 1 + target.len() <= len && chars[i + 1..i + 1 + target.len()] == target {
                let mut m = i + 1 + target.len();
                let mut key = String::new();
                while m < len {
                    let ch = chars[m];
                    if ch.is_ascii_alphanumeric() || ch == '-' {
                        key.push(ch);
                        m += 1;
                    } else {
                        break;
                    }
                }
                if !key.is_empty() {
                    keys.insert(key);
                }
                i = m;
                continue;
            }
        }

        i += 1;
    }
    keys
}

#[cfg(feature = "cli")]
fn extract_typst_labels(input: &str) -> std::collections::HashSet<String> {
    let mut labels = std::collections::HashSet::new();
    let bytes = input.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() {
                let ch = bytes[j] as char;
                if ch == '>' {
                    if j > start {
                        let raw = &input[start..j];
                        if raw
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                        {
                            labels.insert(raw.to_string());
                        }
                    }
                    i = j + 1;
                    break;
                }
                if ch.is_whitespace() {
                    break;
                }
                j += 1;
            }
        }
        i += 1;
    }
    labels
}

#[cfg(feature = "cli")]
fn extract_bibitem_keys_from_typst(input: &str) -> std::collections::HashSet<String> {
    let mut keys = std::collections::HashSet::new();
    let bytes = input.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' && input[i..].starts_with("\\bibitem") {
            let mut j = i + "\\bibitem".len();
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'[' {
                j += 1;
                while j < bytes.len() && bytes[j] != b']' {
                    j += 1;
                }
                if j < bytes.len() {
                    j += 1;
                }
            }
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'{' {
                j += 1;
                let key_start = j;
                while j < bytes.len() && bytes[j] != b'}' {
                    j += 1;
                }
                if j <= bytes.len() {
                    let raw_key = input[key_start..j].trim();
                    let clean = sanitize_citation_key(raw_key);
                    if !clean.is_empty() {
                        keys.insert(clean);
                    }
                }
            }
        }
        i += 1;
    }
    keys
}

#[cfg(feature = "cli")]
fn write_stub_bibliography(
    path: &Path,
    keys: &std::collections::HashSet<String>,
) -> io::Result<()> {
    if keys.is_empty() {
        return Ok(());
    }
    let mut keys_sorted: Vec<&String> = keys.iter().collect();
    keys_sorted.sort();
    let mut stub = String::new();
    for key in keys_sorted {
        stub.push_str("@misc{");
        stub.push_str(key);
        stub.push_str(",\n  title = \"{Missing citation}\",\n  author = \"{Unknown}\",\n  year = \"{0000}\",\n}\n\n");
    }
    fs::write(path, stub)
}

#[cfg(feature = "cli")]
fn populate_placeholder_bibliography(
    out_dir: &Path,
    files: &[String],
    keys: &std::collections::HashSet<String>,
) -> io::Result<()> {
    if keys.is_empty() {
        return Ok(());
    }

    let mut present = std::collections::HashSet::new();
    let mut file_contents: Vec<(String, String)> = Vec::new();
    for file in files {
        let path = out_dir.join(file);
        if !path.exists() {
            file_contents.push((file.clone(), String::new()));
            continue;
        }
        let content = fs::read_to_string(&path).unwrap_or_default();
        for key in keys {
            if !present.contains(key) && bibtex_contains_key(&content, key) {
                present.insert(key.clone());
            }
        }
        file_contents.push((file.clone(), content));
    }

    let mut missing: Vec<&String> = Vec::new();
    for key in keys {
        if !present.contains(key) {
            missing.push(key);
        }
    }

    if missing.is_empty() {
        return Ok(());
    }

    let target_file = match file_contents.first() {
        Some((name, _)) => name,
        None => return Ok(()),
    };
    let target_path = out_dir.join(target_file);
    let mut content = file_contents
        .iter()
        .find(|(name, _)| name == target_file)
        .map(|(_, content)| content.clone())
        .unwrap_or_default();
    let has_entries = content.contains('@');
    let is_placeholder = content.trim().is_empty()
        || content.contains("TYLAX-STUB-BIB")
        || content
            .lines()
            .next()
            .map(|line| line.contains("Missing bibliography source"))
            .unwrap_or(false);

    let mut stub = String::new();
    for key in &missing {
        stub.push_str("@misc{");
        stub.push_str(key);
        stub.push_str(",\n  title = \"{Missing citation}\",\n  author = \"{Unknown}\",\n  year = \"{0000}\",\n}\n\n");
    }
    if is_placeholder || !has_entries {
        fs::write(&target_path, stub)?;
    } else {
        if !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str(&stub);
        fs::write(&target_path, content)?;
    }
    Ok(())
}

#[cfg(feature = "cli")]
fn bibtex_contains_key(content: &str, key: &str) -> bool {
    let bytes = content.as_bytes();
    let key_bytes = key.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'@' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_alphabetic() {
                j += 1;
            }
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && (bytes[j] == b'{' || bytes[j] == b'(') {
                j += 1;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j + key_bytes.len() <= bytes.len() {
                    let slice = &bytes[j..j + key_bytes.len()];
                    if slice.eq_ignore_ascii_case(key_bytes) {
                        return true;
                    }
                }
            }
        }
        i += 1;
    }
    false
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
            let entry_type = String::from_utf8_lossy(&bytes[i + 1..j]).to_string();
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

            let is_meta = entry_type.eq_ignore_ascii_case("string")
                || entry_type.eq_ignore_ascii_case("preamble")
                || entry_type.eq_ignore_ascii_case("comment");

            if is_meta {
                // Scan to matching close without trying to parse a key.
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
                let entry_text = String::from_utf8_lossy(&bytes[start..end]).to_string();
                anon_idx += 1;
                let key_final = format!("{}-{}", entry_type, anon_idx);
                out.push((key_final, entry_text));
                i = end;
                continue;
            }

            // Extract key
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            let key_start = j;
            while j < bytes.len() && bytes[j] != b',' && bytes[j] != close as u8 {
                j += 1;
            }
            let key = String::from_utf8_lossy(&bytes[key_start..j])
                .trim()
                .to_string();

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
            let entry_text = String::from_utf8_lossy(&bytes[start..end]).to_string();

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
fn collect_bibliography_entries_with_includes(input: &str, base_dir: &Path) -> Vec<String> {
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
                if path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .map(|s| s.ends_with(".typst.bib"))
                    .unwrap_or(false)
                {
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
            if path
                .file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.ends_with(".typst.bib"))
                .unwrap_or(false)
            {
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
    let has_ext = raw_path.extension().is_some();

    let mut candidates: Vec<String> = Vec::new();
    candidates.push(raw.to_string());
    if !has_ext {
        for ext in EXTENSIONS {
            let mut with_ext = raw.to_string();
            with_ext.push_str(ext);
            candidates.push(with_ext);
        }
    }

    if raw_path.is_absolute() {
        for candidate in &candidates {
            let candidate_path = PathBuf::from(candidate);
            if candidate_path.exists() {
                let rel = normalize_abs_rel_path(&candidate_path);
                let rel_str = rel.to_string_lossy().to_string();
                return Some((candidate_path, rel_str));
            }
        }
        return None;
    }

    let mut bases: Vec<(PathBuf, PathBuf)> = Vec::new();
    bases.push((base_dir.to_path_buf(), PathBuf::new()));

    for dir in graphic_dirs {
        let trimmed = dir.trim();
        if trimmed.is_empty() {
            continue;
        }
        let candidate = PathBuf::from(trimmed);
        if candidate.is_absolute() {
            if let Ok(rel) = candidate.strip_prefix(base_dir) {
                bases.push((candidate.clone(), normalize_rel_path(rel)));
            } else {
                bases.push((candidate.clone(), normalize_abs_rel_path(&candidate)));
            }
        } else {
            let rel_prefix = normalize_rel_path(&candidate);
            bases.push((base_dir.join(&candidate), rel_prefix));
        }
    }

    for candidate in &candidates {
        for (base, rel_prefix) in &bases {
            let candidate_path = base.join(candidate);
            if candidate_path.exists() {
                let rel = if rel_prefix.as_os_str().is_empty() {
                    normalize_rel_path(Path::new(candidate))
                } else {
                    normalize_rel_path(&rel_prefix.join(candidate))
                };
                let rel_str = rel.to_string_lossy().to_string();
                return Some((candidate_path, rel_str));
            }
        }
    }

    None
}

#[cfg(feature = "cli")]
fn normalize_image_extension(ext: &str) -> String {
    let lower = ext.trim_start_matches('.').to_ascii_lowercase();
    match lower.as_str() {
        "jpeg" | "jpe" => "jpg".to_string(),
        "tif" => "tiff".to_string(),
        other => other.to_string(),
    }
}

#[cfg(feature = "cli")]
fn is_known_image_extension(ext: &str) -> bool {
    matches!(
        normalize_image_extension(ext).as_str(),
        "png" | "jpg" | "gif" | "svg" | "pdf" | "bmp" | "webp" | "tiff"
    )
}

#[cfg(feature = "cli")]
fn detect_image_extension(path: &Path) -> Option<&'static str> {
    let mut file = fs::File::open(path).ok()?;
    let mut buf = [0u8; 512];
    let n = file.read(&mut buf).ok()?;
    let data = &buf[..n];

    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("png");
    }
    if data.starts_with(b"\xFF\xD8\xFF") {
        return Some("jpg");
    }
    if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        return Some("gif");
    }
    if data.starts_with(b"%PDF-") {
        return Some("pdf");
    }
    if data.starts_with(b"BM") {
        return Some("bmp");
    }
    if data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        return Some("webp");
    }

    let text = std::str::from_utf8(data).ok()?;
    let trimmed = text
        .trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '\u{feff}')
        .to_lowercase();
    if trimmed.starts_with("<svg") || (trimmed.starts_with("<?xml") && trimmed.contains("<svg")) {
        return Some("svg");
    }

    None
}

#[cfg(feature = "cli")]
fn normalize_rel_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::RootDir | Component::Prefix(_) => {}
        }
    }
    out
}

#[cfg(feature = "cli")]
fn normalize_abs_rel_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        if let Component::Normal(part) = comp {
            out.push(part);
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from("image")
    } else {
        out
    }
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
            if rel.is_empty() {
                continue;
            }
            let mut final_rel = rel.clone();
            let rel_ext = Path::new(&rel)
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("");

            let detected_ext = detect_image_extension(&src);
            if let Some(actual_ext) = detected_ext {
                let actual_norm = normalize_image_extension(actual_ext);
                let rel_norm = normalize_image_extension(rel_ext);
                if !rel_norm.is_empty() && actual_norm != rel_norm {
                    let mut rel_path = PathBuf::from(&rel);
                    rel_path.set_extension(&actual_norm);
                    final_rel = rel_path.to_string_lossy().to_string();
                }
            }
            // Note: If we can't detect the image type but the file exists (resolve_graphics_path
            // already verified this), we still copy it - don't mark as missing just because
            // we can't read the magic bytes.

            if seen.insert(final_rel.clone()) {
                let dest = out_dir.join(&final_rel);
                if dest == src {
                    continue;
                }
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut copied = fs::copy(&src, &dest).is_ok();
                if copied {
                    if let (Ok(src_meta), Ok(dest_meta)) = (fs::metadata(&src), fs::metadata(&dest))
                    {
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
            if trimmed != final_rel {
                mapping.insert(trimmed.to_string(), final_rel);
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

#[cfg(all(test, feature = "cli"))]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(prefix: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        dir.push(format!("tylax-{}-{}", prefix, ts));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_graphicspath_copy_and_rewrite() {
        let base_dir = temp_dir("graphics-base");
        let out_dir = temp_dir("graphics-out");
        let figs_dir = base_dir.join("figs");
        fs::create_dir_all(&figs_dir).unwrap();
        fs::write(figs_dir.join("plot.png"), b"fake").unwrap();

        let paths = vec!["plot".to_string()];
        let graphic_dirs = vec!["figs/".to_string()];
        let mapping = copy_graphics_assets(&paths, &graphic_dirs, &base_dir, &out_dir).unwrap();

        assert_eq!(
            mapping.get("plot").map(|s| s.as_str()),
            Some("figs/plot.png")
        );
        assert!(out_dir.join("figs").join("plot.png").exists());

        let rewritten = rewrite_image_paths("#image(\"plot\")", &mapping);
        assert!(rewritten.contains("figs/plot.png"));
    }

    #[test]
    fn test_graphicspath_rewrites_mismatched_extension() {
        let base_dir = temp_dir("graphics-mismatch");
        let out_dir = temp_dir("graphics-mismatch-out");
        let imgs_dir = base_dir.join("images");
        fs::create_dir_all(&imgs_dir).unwrap();
        let mut file = fs::File::create(imgs_dir.join("photo.png")).unwrap();
        file.write_all(&[0xFF, 0xD8, 0xFF, 0xE0, b'J', b'F', b'I', b'F', 0x00])
            .unwrap();

        let paths = vec!["images/photo.png".to_string()];
        let mapping = copy_graphics_assets(&paths, &[], &base_dir, &out_dir).unwrap();

        assert_eq!(
            mapping.get("images/photo.png").map(|s| s.as_str()),
            Some("images/photo.jpg")
        );
        assert!(out_dir.join("images").join("photo.jpg").exists());
    }

    #[test]
    fn test_bibliography_copy() {
        let base_dir = temp_dir("bib-base");
        let out_dir = temp_dir("bib-out");
        fs::write(base_dir.join("refs.bib"), "@article{a, title={x}}\n").unwrap();

        let entries = vec!["refs".to_string()];
        let mapping = sanitize_bibliography_files(&entries, &base_dir, &out_dir).unwrap();

        assert_eq!(
            mapping.get("refs.bib").map(|s| s.as_str()),
            Some("refs.typst.bib")
        );
        assert!(out_dir.join("refs.typst.bib").exists());
    }

    #[test]
    fn test_bibliography_fallback_ignores_generated_files() {
        let base_dir = temp_dir("bib-ignore");
        fs::write(base_dir.join("refs.bib"), "@article{a, title={x}}\n").unwrap();
        fs::write(base_dir.join("refs.typst.bib"), "@article{b, title={y}}\n").unwrap();

        let entries = collect_bibliography_entries_with_includes("no bibliography here", &base_dir);

        assert!(entries.iter().any(|e| e == "refs"));
        assert!(!entries.iter().any(|e| e.contains("typst")));
    }
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
                        let mut skip_packages = std::collections::HashSet::new();
                        let mut skipped_list: Vec<String> = Vec::new();
                        for pkg in collect_usepackage_entries(&content) {
                            if is_macro_expansion_blacklisted_package(&pkg) {
                                let lower = pkg.trim().to_lowercase();
                                if skip_packages.insert(lower.clone()) {
                                    skipped_list.push(lower);
                                }
                            }
                        }
                        if !skipped_list.is_empty() {
                            skipped_list.sort();
                            skipped_list.dedup();
                            eprintln!(
                                "⚠ Skipping local package expansion for: {}",
                                skipped_list.join(", ")
                            );
                        }
                        content = expand_local_packages_with_skip(&content, parent, &skip_packages);
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
                        let use_ir =
                            ir || auto_repair || loss_log.is_some() || post_repair_log.is_some();
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
                        let use_ir =
                            ir || auto_repair || loss_log.is_some() || post_repair_log.is_some();
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

            if let Some(output_path) = output.as_ref() {
                let out_dir = Path::new(output_path)
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| std::path::PathBuf::from("."));
                let mut citation_keys = extract_citation_keys_from_typst(&result);
                let bibitem_keys = extract_bibitem_keys_from_typst(&result);
                citation_keys.extend(bibitem_keys.iter().cloned());

                if !bib_entries.is_empty() {
                    if let Some(base_dir) = bib_base_dir.as_ref() {
                        if let Ok(mapping) =
                            sanitize_bibliography_files(&bib_entries, base_dir, &out_dir)
                        {
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
                            let mut mapped_files: Vec<String> = mapping.values().cloned().collect();
                            mapped_files.sort();
                            mapped_files.dedup();
                            let _ = populate_placeholder_bibliography(
                                &out_dir,
                                &mapped_files,
                                &citation_keys,
                            );
                        }
                    }
                } else if !bibitem_keys.is_empty() && !result.contains("#bibliography") {
                    let stub_path = out_dir.join("references.typst.bib");
                    let _ = write_stub_bibliography(&stub_path, &citation_keys);
                    result.push_str("\n#hide(bibliography(\"references.typst.bib\"))\n");
                }
            }
            if let (Some(output_path), Some(base_dir)) =
                (output.as_ref(), graphics_base_dir.as_ref())
            {
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

/// Detect if input is a full LaTeX document (vs math snippet)
#[cfg(feature = "cli")]
fn is_latex_document(input: &str) -> bool {
    // Check for document structure indicators
    input.contains("\\documentclass")
        || input.contains("\\begin{document}")
        || input.contains("\\section")
        || input.contains("\\chapter")
        || input.contains("\\title")
        || input.contains("\\maketitle")
        || input.contains("\\usepackage")
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

/// Print diagnostics to stderr with optional color coding (unified for L2T and T2L).
#[cfg(feature = "cli")]
fn print_diagnostics_to_stderr(diagnostics: &[CliDiagnostic], use_color: bool) {
    eprintln!();
    eprintln!(
        "{}Conversion Warnings ({}):{}",
        if use_color { "\x1b[33m" } else { "" },
        diagnostics.len(),
        if use_color { "\x1b[0m" } else { "" }
    );
    eprintln!();

    for diag in diagnostics {
        let color = if use_color { diag.color_code() } else { "" };
        let reset = if use_color { "\x1b[0m" } else { "" };

        if let Some(ref loc) = diag.location {
            eprintln!(
                "  {}[{}]{} {}: {}",
                color, diag.kind, reset, loc, diag.message
            );
        } else {
            eprintln!("  {}[{}]{} {}", color, diag.kind, reset, diag.message);
        }
    }
    eprintln!();
}

/// Embed diagnostics as comments at the end of the output (unified for L2T and T2L).
#[cfg(feature = "cli")]
fn embed_diagnostics_as_comments(output: &str, diagnostics: &[CliDiagnostic]) -> String {
    let mut result = output.to_string();
    result.push_str("\n\n// ═══════════════════════════════════════════════════════════════\n");
    result.push_str("// Conversion Warnings\n");
    result.push_str("// ═══════════════════════════════════════════════════════════════\n");

    for diag in diagnostics {
        if let Some(ref loc) = diag.location {
            result.push_str(&format!("// [{}] {}: {}\n", diag.kind, loc, diag.message));
        } else {
            result.push_str(&format!("// [{}] {}\n", diag.kind, diag.message));
        }
    }

    result
}

#[cfg(not(feature = "cli"))]
fn main() {
    eprintln!("CLI feature not enabled. Build with --features cli");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("  cargo install tylax --features cli");
    eprintln!("  t2l [OPTIONS] [INPUT_FILE]");
}
