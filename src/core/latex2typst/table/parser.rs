//! State-aware table grid parser

use super::cell::{CellAlign, GridCell};
use super::hline::{clean_cell_content, clean_hline_args, extract_hline_range, HLine};

/// Represents a parsed table row
#[derive(Debug, Clone)]
pub struct GridRow {
    /// Cells in this row
    pub cells: Vec<GridCell>,
    /// Horizontal lines before this row
    pub hlines_before: Vec<HLine>,
}

impl GridRow {
    /// Create a new empty row
    pub fn new() -> Self {
        GridRow {
            cells: Vec::new(),
            hlines_before: Vec::new(),
        }
    }
}

impl Default for GridRow {
    fn default() -> Self {
        Self::new()
    }
}

/// State-aware table grid parser
///
/// This parser maintains a virtual grid state to correctly handle complex
/// LaTeX table features like multirow, multicolumn, and sparse data.
pub struct TableGridParser {
    /// Column coverage tracking: remaining rows each column is covered by a multirow
    col_coverage: Vec<usize>,
    /// Maximum column count observed while parsing rows
    max_cols: usize,
    /// Parsed rows
    pub rows: Vec<GridRow>,
    /// Default column alignments from \begin{tabular}{...}
    pub default_alignments: Vec<CellAlign>,
    /// Pending hlines to attach to the next row
    pending_hlines: Vec<HLine>,
}

impl TableGridParser {
    /// Create a new parser with the given default alignments
    pub fn new(alignments: Vec<CellAlign>) -> Self {
        TableGridParser {
            col_coverage: Vec::new(),
            max_cols: 0,
            rows: Vec::new(),
            default_alignments: alignments,
            pending_hlines: Vec::new(),
        }
    }

    /// Add a full horizontal line
    pub fn add_hline(&mut self) {
        self.pending_hlines.push(HLine::full());
    }

    /// Add a partial horizontal line (cline/cmidrule)
    pub fn add_partial_hline(&mut self, start: usize, end: usize) {
        self.pending_hlines.push(HLine::partial(start, end));
    }

    /// Process a row of raw cells
    pub fn process_row(&mut self, raw_cells: Vec<String>) {
        let mut row = GridRow::new();

        // Attach pending hlines
        row.hlines_before.append(&mut self.pending_hlines);

        let mut input_idx = 0;
        let mut current_col = 0;

        while input_idx < raw_cells.len() {
            // Ensure col_coverage is large enough
            if current_col >= self.col_coverage.len() {
                self.col_coverage.resize(current_col + 1, 0);
            }

            if self.col_coverage[current_col] > 0 {
                // Column is covered by a previous multirow.
                // Check if the input contains a multi-column placeholder (e.g., \multicolumn{2}{c}{}).
                // We must advance current_col by the placeholder's span to correctly align subsequent data.
                let raw = &raw_cells[input_idx];
                let cell = GridCell::parse(raw);
                let is_placeholder = cell.content.trim().is_empty();

                if is_placeholder {
                    // Decrement coverage for active columns.
                    // If a column wasn't covered but is spanned by the placeholder, we ignore it
                    // as it suggests a malformed table structure.
                    let span = cell.colspan.max(1);

                    for i in 0..span {
                        if current_col + i < self.col_coverage.len()
                            && self.col_coverage[current_col + i] > 0
                        {
                            self.col_coverage[current_col + i] -= 1;
                        }
                    }

                    // Consume the placeholder cell from input
                    // but DO NOT emit a cell - Typst handles the spanned area
                    input_idx += 1;
                    current_col += span;
                } else {
                    // If we have real content, treat it as a real cell to avoid dropping data.
                    // Clear existing coverage for the spanned columns and emit the cell.
                    let span = cell.colspan.max(1);
                    if current_col + span > self.col_coverage.len() {
                        self.col_coverage.resize(current_col + span, 0);
                    }
                    for i in 0..span {
                        self.col_coverage[current_col + i] = 0;
                    }

                    let rows_to_cover = cell.rowspan.saturating_sub(1);
                    for i in 0..span {
                        self.col_coverage[current_col + i] = rows_to_cover;
                    }

                    row.cells.push(cell.clone());
                    input_idx += 1;
                    current_col += span;
                }
            } else {
                // Not covered, process the input cell
                let raw = &raw_cells[input_idx];
                let cell = GridCell::parse(raw);

                // Update coverage for future rows
                let rows_to_cover = cell.rowspan.saturating_sub(1);

                // Ensure coverage vec size
                if current_col + cell.colspan > self.col_coverage.len() {
                    self.col_coverage.resize(current_col + cell.colspan, 0);
                }

                // Mark coverage for all columns this cell spans
                for i in 0..cell.colspan {
                    self.col_coverage[current_col + i] = rows_to_cover;
                }

                // Add cell to row (handle backslash artifacts)
                if raw != "\\" {
                    row.cells.push(cell.clone());
                } else {
                    row.cells.push(GridCell::empty());
                }

                input_idx += 1;
                current_col += cell.colspan;
            }
        }

        if current_col > self.max_cols {
            self.max_cols = current_col;
        }

        if !row.cells.is_empty() || !row.hlines_before.is_empty() {
            self.rows.push(row);
        }
    }

    /// Generate Typst table code
    pub fn generate_typst(&self, col_count: usize) -> String {
        use std::fmt::Write;
        let mut output = String::new();

        // Generate columns spec
        let effective_cols = col_count.max(self.max_cols).max(1);
        let col_tuple: Vec<&str> = vec!["auto"; effective_cols];
        let _ = writeln!(output, "#table(");
        let _ = writeln!(output, "    columns: ({}),", col_tuple.join(", "));

        // Generate alignment spec
        if !self.default_alignments.is_empty() {
            let mut aligns = self.default_alignments.clone();
            if aligns.len() < effective_cols {
                aligns.extend(std::iter::repeat(CellAlign::Auto).take(effective_cols - aligns.len()));
            }
            let aligns: Vec<&str> = aligns.iter().map(|a| a.to_typst()).collect();
            let _ = writeln!(output, "    align: ({}),", aligns.join(", "));
        }

        let normalized_rows = self.normalized_rows(effective_cols);

        // Generate rows
        for row in &normalized_rows {
            // Emit hlines before this row
            for hline in &row.hlines_before {
                let _ = writeln!(output, "    {},", hline.to_typst());
            }

            // Emit cells
            if !row.cells.is_empty() {
                let cells_str: Vec<String> = row.cells.iter().map(|c| c.to_typst()).collect();
                let _ = writeln!(output, "    {},", cells_str.join(", "));
            }
        }

        // Emit any remaining pending hlines
        for hline in &self.pending_hlines {
            let _ = writeln!(output, "    {},", hline.to_typst());
        }

        output.push_str(")\n");
        output
    }

    fn normalized_rows(&self, effective_cols: usize) -> Vec<GridRow> {
        let mut normalized = Vec::with_capacity(self.rows.len());
        let mut coverage = vec![0usize; effective_cols];

        for row in &self.rows {
            let mut out_row = GridRow::new();
            out_row.hlines_before = row.hlines_before.clone();

            let mut col = 0usize;
            let mut cell_idx = 0usize;

            while col < effective_cols {
                if coverage[col] > 0 {
                    if cell_idx < row.cells.len() {
                        let pending = &row.cells[cell_idx];
                        let is_placeholder = pending.content.trim().is_empty()
                            && !pending.is_special
                            && pending.rowspan <= 1
                            && pending.colspan <= 1
                            && pending.align.is_none();
                        if is_placeholder {
                            coverage[col] -= 1;
                            col += 1;
                            continue;
                        }
                    } else {
                        coverage[col] -= 1;
                        col += 1;
                        continue;
                    }
                }

                if cell_idx < row.cells.len() {
                    let mut cell = row.cells[cell_idx].clone();
                    if cell.colspan == 0 {
                        cell.colspan = 1;
                    }
                    let remaining = effective_cols - col;
                    if cell.colspan > remaining {
                        cell.colspan = remaining;
                    }

                    let rows_to_cover = cell.rowspan.saturating_sub(1);
                    for i in 0..cell.colspan {
                        if col + i < coverage.len() {
                            coverage[col + i] = rows_to_cover;
                        }
                    }

                    out_row.cells.push(cell);
                    col += out_row.cells.last().map(|c| c.colspan).unwrap_or(1);
                    cell_idx += 1;
                } else {
                    out_row.cells.push(GridCell::empty());
                    col += 1;
                }
            }

            normalized.push(out_row);
        }

        normalized
    }
}

/// Parse table content using the state-aware TableGridParser
pub fn parse_with_grid_parser(content: &str, alignments: Vec<CellAlign>) -> String {
    let col_count = alignments.len().max(1);
    let mut parser = TableGridParser::new(alignments);

    let rows: Vec<&str> = content.split("|||ROW|||").collect();
    let longtable_keep = compute_longtable_keep(&rows);

    for (row_idx, row_str) in rows.iter().enumerate() {
        let row_str = row_str.trim();
        if row_str.is_empty() {
            continue;
        }

        if let Some(ref keep) = longtable_keep {
            if !keep.should_keep(row_idx) {
                continue;
            }
        }

        let has_longtable_control = contains_longtable_control(row_str);

        // Check for HLINE markers and extract partial line info
        if row_str.contains("|||HLINE|||") {
            let hline_info = extract_hline_range(row_str);
            match hline_info {
                Some((start, end)) => parser.add_partial_hline(start, end),
                None => parser.add_hline(),
            }
        }

        // Remove HLINE marker to process content
        let clean_row = row_str.replace("|||HLINE|||", "");
        let clean_row = clean_hline_args(&clean_row);

        if clean_row.trim().is_empty() {
            continue;
        }

        // Split into cells and clean each one
        let raw_cells: Vec<String> = clean_row
            .split("|||CELL|||")
            .map(clean_cell_content)
            .collect();

        let has_cell_markers = clean_row.contains("|||CELL|||");
        if has_longtable_control
            && !has_cell_markers
            && raw_cells.iter().all(|c| c.trim().is_empty())
        {
            continue;
        }

        parser.process_row(raw_cells);
    }

    // Handle single row without ROW markers (edge case)
    if parser.rows.is_empty() && content.contains("|||CELL|||") {
        let clean_content = content.replace("|||HLINE|||", "");
        let raw_cells: Vec<String> = clean_content
            .split("|||CELL|||")
            .map(clean_cell_content)
            .collect();
        parser.process_row(raw_cells);
    }

    parser.generate_typst(col_count)
}

fn contains_longtable_control(s: &str) -> bool {
    let s = s.to_ascii_lowercase();
    s.contains("endfirsthead")
        || s.contains("endhead")
        || s.contains("endfoot")
        || s.contains("endlastfoot")
}

struct LongtableKeep {
    header_end: Option<usize>,
    body_start: usize,
}

impl LongtableKeep {
    fn should_keep(&self, idx: usize) -> bool {
        let in_header = self.header_end.map(|end| idx < end).unwrap_or(false);
        let in_body = idx >= self.body_start;
        in_header || in_body
    }
}

fn compute_longtable_keep(rows: &[&str]) -> Option<LongtableKeep> {
    if !rows.iter().any(|r| contains_longtable_control(r)) {
        return None;
    }

    let endfirsthead = find_control_row(rows, "endfirsthead");
    let endhead = find_control_row(rows, "endhead");
    let endfoot = find_control_row(rows, "endfoot");
    let endlastfoot = find_control_row(rows, "endlastfoot");

    let header_end = endfirsthead.or(endhead);
    let body_start = if let Some(idx) = endlastfoot {
        idx + 1
    } else if let Some(idx) = endfoot {
        idx + 1
    } else if let Some(idx) = endhead {
        idx + 1
    } else if let Some(idx) = endfirsthead {
        idx + 1
    } else {
        0
    };

    Some(LongtableKeep {
        header_end,
        body_start,
    })
}

fn find_control_row(rows: &[&str], keyword: &str) -> Option<usize> {
    rows.iter().enumerate().find_map(|(idx, row)| {
        let row_lower = row.to_ascii_lowercase();
        if row_lower.contains(keyword) && is_longtable_control_row(row) {
            Some(idx)
        } else {
            None
        }
    })
}

fn is_longtable_control_row(row: &str) -> bool {
    if !contains_longtable_control(row) {
        return false;
    }

    let clean_row = row.replace("|||HLINE|||", "");
    let clean_row = clean_hline_args(&clean_row);
    let raw_cells: Vec<String> = clean_row
        .split("|||CELL|||")
        .map(clean_cell_content)
        .collect();
    raw_cells.iter().all(|c| c.trim().is_empty())
}
