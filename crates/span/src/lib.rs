//! Source locations and compiler diagnostics.

use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BytePos(pub u32);

impl BytePos {
    pub fn to_usize(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Span {
    pub lo: BytePos,
    pub hi: BytePos,
}

impl Span {
    pub const DUMMY: Span = Span {
        lo: BytePos(0),
        hi: BytePos(0),
    };

    pub fn new(lo: u32, hi: u32) -> Self {
        Span {
            lo: BytePos(lo),
            hi: BytePos(hi),
        }
    }

    pub fn is_dummy(self) -> bool {
        self.lo.0 == 0 && self.hi.0 == 0
    }

    pub fn merge(self, other: Span) -> Span {
        if self.is_dummy() {
            return other;
        }
        if other.is_dummy() {
            return self;
        }
        Span {
            lo: BytePos(self.lo.0.min(other.lo.0)),
            hi: BytePos(self.hi.0.max(other.hi.0)),
        }
    }

    pub fn len(self) -> u32 {
        self.hi.0.saturating_sub(self.lo.0)
    }
}

#[derive(Clone, Debug)]
pub struct SourceFile {
    pub name: String,
    pub path: PathBuf,
    pub src: String,
    /// Byte offset of the start of each line (0-based line index).
    lines: Vec<u32>,
}

impl SourceFile {
    pub fn new(name: impl Into<String>, src: impl Into<String>) -> Self {
        let name = name.into();
        let src = src.into();
        let path = PathBuf::from(&name);
        Self::from_path(path, name, src)
    }

    pub fn from_path(path: PathBuf, name: String, src: String) -> Self {
        let mut lines = vec![0];
        for (i, b) in src.bytes().enumerate() {
            if b == b'\n' {
                lines.push((i + 1) as u32);
            }
        }
        SourceFile {
            name,
            path,
            src,
            lines,
        }
    }

    pub fn load(path: &Path) -> std::io::Result<Self> {
        let src = std::fs::read_to_string(path)?;
        let name = path.display().to_string();
        Ok(Self::from_path(path.to_path_buf(), name, src))
    }

    pub fn span_text(&self, span: Span) -> &str {
        let lo = (span.lo.0 as usize).min(self.src.len());
        let hi = (span.hi.0 as usize).min(self.src.len());
        if lo > hi {
            return "";
        }
        &self.src[lo..hi]
    }

    /// 1-based line and column for a byte position.
    pub fn loc(&self, pos: BytePos) -> (u32, u32) {
        let off = pos.0;
        let mut lo = 0usize;
        let mut hi = self.lines.len();
        while lo + 1 < hi {
            let mid = (lo + hi) / 2;
            if self.lines[mid] <= off {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let line_start = self.lines[lo];
        let col = off.saturating_sub(line_start) + 1;
        ((lo as u32) + 1, col)
    }

    pub fn line_text(&self, line_1based: u32) -> &str {
        let idx = line_1based.saturating_sub(1) as usize;
        if idx >= self.lines.len() {
            return "";
        }
        let start = self.lines[idx] as usize;
        let end = if idx + 1 < self.lines.len() {
            self.lines[idx + 1] as usize
        } else {
            self.src.len()
        };
        let mut slice = &self.src[start.min(self.src.len())..end.min(self.src.len())];
        if let Some(s) = slice.strip_suffix('\n') {
            slice = s;
        }
        if let Some(s) = slice.strip_suffix('\r') {
            slice = s;
        }
        slice
    }

    pub fn num_lines(&self) -> usize {
        self.lines.len()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Level {
    Error,
    Warning,
    Note,
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Level::Error => write!(f, "error"),
            Level::Warning => write!(f, "warning"),
            Level::Note => write!(f, "note"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub level: Level,
    pub message: String,
    pub span: Span,
    pub file_index: usize,
    pub notes: Vec<String>,
}

impl Diagnostic {
    pub fn error(file_index: usize, span: Span, message: impl Into<String>) -> Self {
        Diagnostic {
            level: Level::Error,
            message: message.into(),
            span,
            file_index,
            notes: Vec::new(),
        }
    }

    pub fn warning(file_index: usize, span: Span, message: impl Into<String>) -> Self {
        Diagnostic {
            level: Level::Warning,
            message: message.into(),
            span,
            file_index,
            notes: Vec::new(),
        }
    }

    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub fn render(&self, sources: &[SourceFile]) -> String {
        let mut out = String::new();
        let file = sources.get(self.file_index);
        out.push_str(&format!("{}: {}\n", self.level, self.message));
        if let Some(file) = file {
            if !self.span.is_dummy() {
                let (line, col) = file.loc(self.span.lo);
                out.push_str(&format!("  --> {}:{}:{}\n", file.name, line, col));
                let line_txt = file.line_text(line);
                let line_no = format!("{line}");
                let pad = " ".repeat(line_no.len());
                out.push_str(&format!("   {pad} |\n"));
                out.push_str(&format!(" {line_no} | {line_txt}\n"));
                let caret_col = col.saturating_sub(1) as usize;
                let width = (self.span.len() as usize).max(1);
                // Don't wrap past the line.
                let width = width.min(line_txt.len().saturating_sub(caret_col).max(1));
                out.push_str(&format!(
                    "   {pad} | {}{}\n",
                    " ".repeat(caret_col),
                    "^".repeat(width)
                ));
            } else {
                out.push_str(&format!("  --> {}\n", file.name));
            }
        }
        for n in &self.notes {
            out.push_str(&format!("   = note: {n}\n"));
        }
        out
    }
}

pub fn render_all(diags: &[Diagnostic], sources: &[SourceFile]) -> String {
    let mut s = String::new();
    for d in diags {
        s.push_str(&d.render(sources));
        if !s.ends_with('\n') {
            s.push('\n');
        }
        s.push('\n');
    }
    let errors = diags.iter().filter(|d| d.level == Level::Error).count();
    let warnings = diags.iter().filter(|d| d.level == Level::Warning).count();
    if errors > 0 || warnings > 0 {
        s.push_str(&format!("{errors} error(s), {warnings} warning(s)\n"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loc_basic() {
        let sf = SourceFile::new("t.scala", "ab\ncd\n");
        assert_eq!(sf.loc(BytePos(0)), (1, 1));
        assert_eq!(sf.loc(BytePos(2)), (1, 3));
        assert_eq!(sf.loc(BytePos(3)), (2, 1));
        assert_eq!(sf.line_text(1), "ab");
        assert_eq!(sf.line_text(2), "cd");
    }

    #[test]
    fn diagnostic_render_contains_caret() {
        let sf = SourceFile::new("t.scala", "val x = foo\n");
        let d = Diagnostic::error(0, Span::new(8, 11), "not found: value foo");
        let s = d.render(&[sf]);
        assert!(s.contains("error: not found: value foo"));
        assert!(s.contains("t.scala:1:9"));
        assert!(s.contains("^^^"));
    }
}
