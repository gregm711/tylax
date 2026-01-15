//! Semantic intermediate representation for document conversion.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    pub blocks: Vec<Block>,
    pub losses: Vec<Loss>,
}

impl Document {
    pub fn new(blocks: Vec<Block>) -> Self {
        Self {
            blocks,
            losses: Vec::new(),
        }
    }

    pub fn with_losses(blocks: Vec<Block>, losses: Vec<Loss>) -> Self {
        Self { blocks, losses }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Paragraph(Vec<Inline>),
    VSpace(String),
    Heading {
        level: u8,
        content: Vec<Inline>,
        numbered: bool,
    },
    List { kind: ListKind, items: Vec<Vec<Block>> },
    MathBlock(MathBlock),
    CodeBlock(String),
    Quote(Vec<Block>),
    Align { alignment: Alignment, blocks: Vec<Block> },
    Table(Table),
    Figure(Figure),
    Environment(EnvironmentBlock),
    Bibliography { file: String, style: Option<String> },
    Outline { title: Option<Vec<Inline>> },
    Box(BoxBlock),
    Block(BlockBlock),
    Columns(Columns),
    Grid(Grid),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub columns: usize,
    pub cells: Vec<TableCell>,
    pub align: Option<Vec<Alignment>>,
    pub caption: Option<Vec<Inline>>,
    pub stroke: Option<String>,
    pub fill: Option<String>,
    pub inset: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableCell {
    pub content: Vec<Inline>,
    pub colspan: usize,
    pub rowspan: usize,
    pub align: Option<Alignment>,
    pub is_header: bool,
    pub fill: Option<String>,
    pub stroke: Option<String>,
    pub inset: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathBlock {
    pub content: String,
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Figure {
    pub content: FigureContent,
    pub caption: Option<Vec<Inline>>,
    pub label: Option<String>,
    pub placement: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentBlock {
    pub name: String,
    pub title: Option<Vec<Inline>>,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FigureContent {
    Table(Table),
    Image(Image),
    Raw(Vec<Block>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub path: String,
    pub width: Option<String>,
    pub height: Option<String>,
    pub fit: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKind {
    Unordered,
    Ordered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inline {
    Text(String),
    Size { size: String, content: Vec<Inline> },
    Strong(Vec<Inline>),
    Emph(Vec<Inline>),
    Code(String),
    Math(String),
    Link { text: Vec<Inline>, url: String },
    Ref(String),
    Label(String),
    Cite(String),
    Footnote(Vec<Inline>),
    Color { color: String, content: Vec<Inline> },
    RawLatex(String),
    Superscript(Vec<Inline>),
    Subscript(Vec<Inline>),
    LineBreak,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoxBlock {
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockBlock {
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Columns {
    pub columns: usize,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grid {
    pub columns: usize,
    pub cells: Vec<Vec<Block>>,
    pub gutter: Option<String>,
    pub row_gutter: Option<String>,
    pub column_gutter: Option<String>,
}

impl Inline {
    pub fn text(s: impl Into<String>) -> Self {
        Inline::Text(s.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loss {
    pub kind: String,
    pub message: String,
}

impl Loss {
    pub fn new(kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            message: message.into(),
        }
    }
}
