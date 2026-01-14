use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tylax::typst_to_latex_ir;

#[test]
fn pdf_diff_templates() {
    if env::var("PDF_DIFF").ok().as_deref() != Some("1") {
        eprintln!("PDF diff test skipped (set PDF_DIFF=1 to enable)");
        return;
    }

    for tool in ["typst", "tectonic", "pdftoppm", "compare"] {
        if !has_tool(tool) {
            eprintln!("PDF diff test skipped (missing tool: {})", tool);
            return;
        }
    }

    let templates = vec![
        ("ieee", PathBuf::from("../public/templates/ieee.typ")),
        ("arxiv", PathBuf::from("../public/templates/arxiv.typ")),
        ("paper", PathBuf::from("../public/templates/paper.typ")),
        ("report", PathBuf::from("../public/templates/report.typ")),
        ("thesis", PathBuf::from("../public/templates/thesis.typ")),
        ("lab-report", PathBuf::from("../public/templates/lab-report.typ")),
    ];

    let thresholds = load_thresholds(Path::new("tests/pdf_diff_thresholds.txt"));

    let out_root = PathBuf::from("target/pdf_diff");
    fs::create_dir_all(&out_root).expect("create diff output root");

    let mut results: BTreeMap<String, f64> = BTreeMap::new();

    for (name, typst_path) in templates {
        if !typst_path.exists() {
            eprintln!("Skipping missing template: {}", typst_path.display());
            continue;
        }

        let output_dir = out_root.join(name);
        fs::create_dir_all(&output_dir).expect("create template output dir");

        let typst_pdf = output_dir.join(format!("{}.typst.pdf", name));
        let latex_pdf = output_dir.join(format!("{}.latex.pdf", name));
        let tex_path = output_dir.join(format!("{}.tex", name));

        let typst_source = fs::read_to_string(&typst_path)
            .unwrap_or_else(|_| panic!("failed to read {}", typst_path.display()));
        let latex = typst_to_latex_ir(&typst_source, true);
        fs::write(&tex_path, latex).expect("write latex output");

        compile_typst(&typst_path, &typst_pdf, typst_path.parent().unwrap())
            .unwrap_or_else(|err| panic!("typst compile failed for {}: {}", name, err));
        compile_latex(&tex_path, &output_dir)
            .map(|pdf| {
                if pdf != latex_pdf {
                    let _ = fs::rename(pdf, &latex_pdf);
                }
            })
            .unwrap_or_else(|err| panic!("latex compile failed for {}: {}", name, err));

        let typst_png_dir = output_dir.join("typst_png");
        let latex_png_dir = output_dir.join("latex_png");
        let _ = fs::remove_dir_all(&typst_png_dir);
        let _ = fs::remove_dir_all(&latex_png_dir);
        fs::create_dir_all(&typst_png_dir).expect("create typst png dir");
        fs::create_dir_all(&latex_png_dir).expect("create latex png dir");

        let typst_pages = render_pdf_to_png(&typst_pdf, &typst_png_dir)
            .unwrap_or_else(|err| panic!("render typst pdf failed for {}: {}", name, err));
        let latex_pages = render_pdf_to_png(&latex_pdf, &latex_png_dir)
            .unwrap_or_else(|err| panic!("render latex pdf failed for {}: {}", name, err));

        assert_eq!(
            typst_pages.len(),
            latex_pages.len(),
            "page count mismatch for {} (typst {}, latex {})",
            name,
            typst_pages.len(),
            latex_pages.len()
        );

        let mut max_rmse = 0.0;
        for (idx, (typst_png, latex_png)) in
            typst_pages.iter().zip(latex_pages.iter()).enumerate()
        {
            let diff_path = output_dir.join(format!("diff-{}.png", idx + 1));
            let rmse = compare_images(typst_png, latex_png, &diff_path)
                .unwrap_or_else(|err| panic!("compare failed for {} page {}: {}", name, idx + 1, err));
            if rmse > max_rmse {
                max_rmse = rmse;
            }
        }

        results.insert(name.to_string(), max_rmse);
        let threshold = thresholds.get(name).copied().unwrap_or(0.25);
        assert!(
            max_rmse <= threshold,
            "{} diff too large: rmse {:.6} > threshold {:.6}",
            name,
            max_rmse,
            threshold
        );
    }

    eprintln!("PDF diff RMSE summary: {:?}", results);
}

fn has_tool(name: &str) -> bool {
    Command::new("command")
        .arg("-v")
        .arg(name)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn compile_typst(input: &Path, output: &Path, root: &Path) -> Result<(), String> {
    let status = Command::new("typst")
        .arg("compile")
        .arg("--root")
        .arg(root)
        .arg(input)
        .arg(output)
        .status()
        .map_err(|err| err.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("typst exited with {}", status))
    }
}

fn compile_latex(input: &Path, out_dir: &Path) -> Result<PathBuf, String> {
    let status = Command::new("tectonic")
        .arg("-X")
        .arg("compile")
        .arg(input)
        .arg("--outdir")
        .arg(out_dir)
        .status()
        .map_err(|err| err.to_string())?;
    if !status.success() {
        return Err(format!("tectonic exited with {}", status));
    }
    let mut pdf = input.to_path_buf();
    pdf.set_extension("pdf");
    if pdf.exists() {
        Ok(pdf)
    } else {
        let filename = input
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| "missing tex file name".to_string())?;
        Ok(out_dir.join(format!("{}.pdf", filename)))
    }
}

fn render_pdf_to_png(pdf: &Path, out_dir: &Path) -> Result<Vec<PathBuf>, String> {
    let prefix = out_dir.join("page");
    let status = Command::new("pdftoppm")
        .arg("-png")
        .arg(pdf)
        .arg(&prefix)
        .status()
        .map_err(|err| err.to_string())?;
    if !status.success() {
        return Err(format!("pdftoppm exited with {}", status));
    }
    let mut pages: Vec<PathBuf> = fs::read_dir(out_dir)
        .map_err(|err| err.to_string())?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("png"))
        .collect();
    pages.sort();
    Ok(pages)
}

fn compare_images(a: &Path, b: &Path, diff: &Path) -> Result<f64, String> {
    let output = Command::new("compare")
        .arg("-metric")
        .arg("RMSE")
        .arg(a)
        .arg(b)
        .arg(diff)
        .output()
        .map_err(|err| err.to_string())?;
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    parse_rmse(&stderr).ok_or_else(|| format!("failed to parse RMSE from: {}", stderr))
}

fn parse_rmse(raw: &str) -> Option<f64> {
    let start = raw.rfind('(')? + 1;
    let end = raw.rfind(')')?;
    raw.get(start..end)?.trim().parse::<f64>().ok()
}

fn load_thresholds(path: &Path) -> BTreeMap<String, f64> {
    let mut out = BTreeMap::new();
    let Ok(contents) = fs::read_to_string(path) else {
        return out;
    };
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        if let Ok(val) = value.trim().parse::<f64>() {
            out.insert(name.trim().to_string(), val);
        }
    }
    out
}
