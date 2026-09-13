// SPDX-License-Identifier: Apache-2.0
//
// render.rs -- a diagnostic as a compiler prints it: the message, the
// place, the offending line with the construct underlined, and what to do
// about it (#457).
//
// Hand-rolled rather than taken from a crate. The hard part of this was
// mapping a merged-buffer offset back to a real file position, and #456
// did that; drawing a caret under a span is the easy half, and doing it
// here keeps the crate at four dependencies.
//
// The snippet text comes from the **merged buffer**, not from the file on
// disk. It needs no I/O, it is always available, and for a spliced
// segment it is verbatim what was written. One consequence, accepted
// rather than hidden: inside a region
// `parse::repair_bare_macro_statements` rewrote, the text shown can
// differ from the file by the one byte the repair overwrote, while the
// position stays right.

use crate::model::Diagnostic;

/// Width a tab advances to, matching `.clang-format`'s 8-space tab.
const TAB_WIDTH: usize = 8;

/// The prefix on the first line. `main.rs` owns the one for its own
/// `oz_err!`; this is the same text, for the renderer's own first line.
const PREFIX: &str = "oz2c error: ";

/// How the diagnostic reads on a terminal.
///
/// ```text
/// oz2c error: '-retain' cannot be sent here -- ARC owns this reference
///   --> src/Keyboard.m:7:9
///    |
///  7 |         [obj retain];
///    |         ^^^^^^^^^^^^
///    |
/// note: ARC is always enabled in the static subset
/// help: let scope-based ARC manage the lifetime
/// ```
///
/// The first line is self-contained on purpose:
/// `tests/tools/oz_static_build.py` reports the first stderr line as the
/// reason a transpile failed, so the summary has to stand alone.
///
/// A diagnostic with no span, or one whose position could not be
/// resolved to a file, degrades to the one-line form rather than drawing
/// a frame around nothing.
pub fn render(diag: &Diagnostic, merged: &str) -> String {
    let mut out = format!("{}{}\n", PREFIX, diag.message);

    if let Some(frame) = frame(diag, merged) {
        out.push_str(&frame);
    }
    if let Some(note) = &diag.note {
        out.push_str(&wrapped("note: ", note));
    }
    for help in &diag.help {
        out.push_str(&wrapped("help: ", help));
    }
    out
}

/// The `--> file:line:col` header, the source line, and the underline.
///
/// `None` when there is nothing honest to draw: no span, no resolved
/// file, or a span whose line cannot be recovered from the buffer.
fn frame(diag: &Diagnostic, merged: &str) -> Option<String> {
    let span = diag.span.as_ref()?;
    let file = diag.file.as_ref()?;
    let (line_text, line_start) = line_containing(merged, span.start)?;

    /* Columns are counted in *display* width, not bytes: 58 of the 81
     * behaviour cases are tab-indented, and a caret padded with one
     * space per byte lands nowhere near the construct on any of them. */
    let lead = &line_text[..span.start.saturating_sub(line_start).min(line_text.len())];
    let caret_col = display_width(lead);

    /* Clipped to the first line. A rejection can span a whole method
     * body, and underlining forty lines buries the `help:` that says
     * what to do -- so the span is drawn to the end of this line and a
     * marker says it continues. */
    let end_in_line = span.end.saturating_sub(line_start).min(line_text.len());
    let spanned = &line_text[span.start.saturating_sub(line_start).min(line_text.len())
        ..end_in_line.max(span.start.saturating_sub(line_start).min(line_text.len()))];
    let clipped = span.end.saturating_sub(line_start) > line_text.len();
    let width = display_width(spanned).max(1);

    let number = diag.line.to_string();
    let gutter = " ".repeat(number.len());
    let shown = expand_tabs(line_text);

    let mut frame = String::new();
    /* `-->` is indented by the gutter width, which puts it one column
     * left of the bar -- rustc's exact shape, so the output reads as a
     * compiler's rather than as an approximation of one. */
    frame.push_str(&format!(
        "{}--> {}:{}:{}\n",
        " ".repeat(number.len()),
        file.display(),
        diag.line,
        diag.col
    ));
    frame.push_str(&format!("{} |\n", gutter));
    frame.push_str(&format!("{} | {}\n", number, shown));
    frame.push_str(&format!(
        "{} | {}{}{}\n",
        gutter,
        " ".repeat(caret_col),
        "^".repeat(width),
        if clipped { " ..." } else { "" }
    ));
    if diag.note.is_some() || !diag.help.is_empty() {
        frame.push_str(&format!("{} |\n", gutter));
    }
    Some(frame)
}

/// The line holding `offset`, and the buffer offset that line starts at.
fn line_containing(merged: &str, offset: usize) -> Option<(&str, usize)> {
    if offset > merged.len() {
        return None;
    }
    let start = merged[..offset].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let end = merged[start..].find('\n').map(|i| start + i).unwrap_or(merged.len());
    Some((&merged[start..end], start))
}

/// Display width of `text`, counting a tab as its advance to the next
/// tab stop and everything else as one column.
///
/// Chars rather than bytes, so a non-ASCII identifier or a `@"…"`
/// literal ahead of the span does not push the caret right by the length
/// of its UTF-8 encoding.
fn display_width(text: &str) -> usize {
    let mut w = 0;
    for c in text.chars() {
        if c == '\t' {
            w += TAB_WIDTH - (w % TAB_WIDTH);
        } else {
            w += 1;
        }
    }
    w
}

/// `text` with tabs expanded, so the printed line agrees with the
/// caret row beneath it however the terminal renders a tab.
fn expand_tabs(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c == '\t' {
            let pad = TAB_WIDTH - (display_width(&out) % TAB_WIDTH);
            out.push_str(&" ".repeat(pad));
        } else {
            out.push(c);
        }
    }
    out
}

/// A `note:`/`help:` line, wrapped to 78 columns with continuations
/// indented under the label so the tier stays readable at terminal width.
fn wrapped(label: &str, body: &str) -> String {
    const WIDTH: usize = 78;
    let indent = " ".repeat(label.len());
    let mut out = String::new();
    let mut col = 0;
    for word in body.split_whitespace() {
        if col == 0 {
            out.push_str(if out.is_empty() { label } else { &indent });
            out.push_str(word);
            col = label.len() + word.chars().count();
        } else if col + 1 + word.chars().count() > WIDTH {
            out.push('\n');
            out.push_str(&indent);
            out.push_str(word);
            col = label.len() + word.chars().count();
        } else {
            out.push(' ');
            out.push_str(word);
            col += 1 + word.chars().count();
        }
    }
    out.push('\n');
    out
}
