//! The abstract syntax tree produced by the parser.

use crate::diagnostics::Span;
use std::cell::Cell;
use std::fmt;
use std::rc::Rc;

/// Where a variable lives. The interpreter's resolver fills this in before a
/// program runs, so variables are read and written by position, not by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Res {
    #[default]
    Unresolved,
    /// A slot in the environment `depth` levels up (0 = the current function).
    Local { depth: u16, index: u32 },
    /// A built-in (standard library) value.
    Global(u32),
    /// Not defined anywhere the program can see.
    Unknown,
}

#[derive(Debug, Clone)]
pub struct Program {
    pub body: Block,
}

pub type Block = Vec<Stmt>;

#[derive(Debug, Clone, PartialEq)]
pub struct Name {
    pub text: String,
    pub span: Span,
    pub res: Cell<Res>,
}

#[derive(Debug, Clone)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum StmtKind {
    Expr(Expr),
    Show(Vec<Expr>),
    /// `x = 1`, `x += 1`, `name: String = "x"`, `const appName = "LiPi"`, `user.name = "x"`
    Assign { target: Target, op: Option<BinOp>, ty: Option<TypeExpr>, value: Expr, constant: bool },
    If { branches: Vec<(Expr, Block)>, otherwise: Option<Block> },
    While { cond: Expr, body: Block },
    /// `for item in items` or `for key, value in object`
    For { first: Name, second: Option<Name>, iter: Expr, body: Block },
    Repeat { count: Expr, body: Block },
    Func(Rc<FuncDecl>),
    Return(Option<Expr>),
    Break,
    Continue,
    Throw(Expr),
    Try { body: Block, catch: Option<(Option<Name>, Block)>, finally: Option<Block> },
    Match { subject: Expr, arms: Vec<MatchArm>, otherwise: Option<Block> },
    /// `use math`, `use "./helpers.lipi" as h`, `from math use add, sub`
    Use { source: String, alias: Option<Name>, names: Option<Vec<Name>> },
    /// `export add, sub` or `export <definition>`
    Export { names: Vec<Name>, inner: Option<Box<Stmt>> },
    TypeDef(Rc<TypeDecl>),
    Test { name: String, body: Block },
    /// `component ProductCard(product)` + block: a function that draws part of a page.
    Component(Rc<FuncDecl>),
    /// `state count = 0`: a variable whose changes redraw the page. Inside a
    /// component it belongs to that component instance and survives redraws.
    State { name: Name, ty: Option<TypeExpr>, value: Expr },
}

#[derive(Debug, Clone)]
pub enum Target {
    Name(Name),
    Field(Expr, Name),
    Index(Expr, Expr),
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub patterns: Vec<Expr>,
    pub guard: Option<Expr>,
    pub body: Block,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: Name,
    pub ty: Option<TypeExpr>,
    pub default: Option<Expr>,
}

#[derive(Debug, Clone)]
pub struct FuncDecl {
    pub name: Name,
    pub params: Vec<Param>,
    pub ret: Option<TypeExpr>,
    pub body: Block,
    pub is_async: bool,
    /// Lambdas (`x => x * 2`) and trailing blocks: they inherit their surroundings' async context.
    pub is_lambda: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct FieldDecl {
    pub name: Name,
    pub ty: Option<TypeExpr>,
    pub default: Option<Expr>,
}

#[derive(Debug, Clone)]
pub struct TypeDecl {
    pub name: Name,
    pub fields: Vec<FieldDecl>,
    pub methods: Vec<Rc<FuncDecl>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeExpr {
    pub kind: TypeKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeKind {
    Named(String),
    List(Box<TypeExpr>),
    Optional(Box<TypeExpr>),
}

impl fmt::Display for TypeExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            TypeKind::Named(n) => write!(f, "{n}"),
            TypeKind::List(inner) => write!(f, "Array[{inner}]"),
            TypeKind::Optional(inner) => write!(f, "{inner}?"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
    /// For `Ident`: where the variable lives (see `Res`).
    pub res: Cell<Res>,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Int(i64),
    Decimal(f64),
    Str(String),
    Template(Vec<TemplatePart>),
    Bool(bool),
    Null,
    Ident(String),
    List(Vec<Expr>),
    Object(Vec<(Name, Expr)>),
    Unary(UnaryOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    /// `a ?? b` — `a` unless it is null
    Coalesce(Box<Expr>, Box<Expr>),
    /// `"adult" if age >= 18 else "minor"`
    IfElse { cond: Box<Expr>, then: Box<Expr>, otherwise: Box<Expr> },
    /// `1 to 10 step 2` (inclusive)
    Range { start: Box<Expr>, end: Box<Expr>, step: Option<Box<Expr>> },
    Call { callee: Box<Expr>, args: Vec<Arg> },
    Field { object: Box<Expr>, name: Name, optional: bool },
    Index { object: Box<Expr>, index: Box<Expr> },
    Lambda(Rc<FuncDecl>),
    Await(Box<Expr>),
}

#[derive(Debug, Clone)]
pub enum TemplatePart {
    Lit(String),
    Expr(Expr),
}

#[derive(Debug, Clone)]
pub struct Arg {
    pub name: Option<Name>,
    pub value: Expr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Eq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    In,
    NotIn,
}

impl BinOp {
    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Mod => "%",
            BinOp::Pow => "**",
            BinOp::Eq => "==",
            BinOp::NotEq => "!=",
            BinOp::Lt => "<",
            BinOp::Gt => ">",
            BinOp::LtEq => "<=",
            BinOp::GtEq => ">=",
            BinOp::In => "in",
            BinOp::NotIn => "not in",
        }
    }

    pub fn is_arithmetic(self) -> bool {
        matches!(self, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod | BinOp::Pow)
    }

    pub fn is_ordering(self) -> bool {
        matches!(self, BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq)
    }
}
