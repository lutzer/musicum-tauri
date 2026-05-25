use crossterm::terminal;
use serde::Serialize;

pub fn print_json<T: Serialize>(value: &T) {
    let json = serde_json::to_string_pretty(value).unwrap();
    let term_w = terminal::size().map(|(w, _)| w as usize).unwrap_or(80);
    let inner_w = term_w.saturating_sub(4); // 2 for "│ " + 2 for " │"
    let top = format!("┌{}┐", "─".repeat(term_w - 2));
    let bot = format!("└{}┘", "─".repeat(term_w - 2));
    println!("{top}");
    for line in json.lines() {
        let char_count = line.chars().count();
        if char_count <= inner_w {
            println!("│ {line:<inner_w$} │");
        } else {
            let truncated: String = line.chars().take(inner_w.saturating_sub(1)).collect();
            println!("│ {truncated}… │");
        }
    }
    println!("{bot}");
}

pub enum DetailItem<'a> {
    Field(&'a str, String),
    Section(&'a str),
}

pub fn print_section_header(title: &str) {
    let term_w = terminal::size().map(|(w, _)| w as usize).unwrap_or(80);
    let prefix = format!("── {title} ");
    let dashes = "─".repeat(term_w.saturating_sub(prefix.chars().count()));
    println!("\n{prefix}{dashes}");
}

pub fn print_table(title: &str, headers: &[&str], rows: Vec<Vec<String>>) {
    let term_w = terminal::size().map(|(w, _)| w as usize).unwrap_or(80);
    print_section_header(title);
    print!("{}", format_table(headers, &rows, term_w));
}

pub fn print_detail(items: &[DetailItem]) {
    let key_w = items
        .iter()
        .filter_map(|i| if let DetailItem::Field(k, _) = i { Some(k.len()) } else { None })
        .max()
        .unwrap_or(0);
    for item in items {
        match item {
            DetailItem::Field(key, val) => println!("{key:>key_w$}: {val}"),
            DetailItem::Section(title) => print_section_header(title),
        }
    }
}

pub fn print_result(action: &str, items: &[DetailItem]) {
    println!("{action}");
    if !items.is_empty() {
        print_detail(items);
    }
}

fn format_table(headers: &[&str], rows: &[Vec<String>], term_w: usize) -> String {
    let ncols = headers.len();
    if ncols == 0 {
        return String::new();
    }

    // Base widths from content
    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    for row in rows {
        for (w, cell) in widths.iter_mut().zip(row.iter()) {
            *w = (*w).max(cell.len());
        }
    }

    let separators = (ncols - 1) * 2;
    let total_base: usize = widths.iter().sum::<usize>() + separators;
    if total_base <= term_w {
        // Distribute remaining space equally among all columns
        let extra = term_w - total_base;
        let per_col = extra / ncols;
        let leftover = extra % ncols;
        for (i, w) in widths.iter_mut().enumerate() {
            *w += per_col + if i < leftover { 1 } else { 0 };
        }
    } else {
        // Content wider than terminal: cap last column to remaining space
        let fixed: usize = widths[..ncols - 1].iter().sum::<usize>() + separators;
        widths[ncols - 1] = term_w.saturating_sub(fixed).max(headers[ncols - 1].len());
    }

    let mut out = String::new();

    let append_row = |out: &mut String, cells: &[&str]| {
        for (i, (width, cell)) in widths.iter().zip(cells.iter()).enumerate() {
            if i > 0 {
                out.push_str("  ");
            }
            let w = *width;
            let char_count = cell.chars().count();
            if char_count <= w {
                out.push_str(&format!("{cell:<w$}"));
            } else {
                let truncated: String = cell.chars().take(w.saturating_sub(1)).collect();
                out.push_str(&truncated);
                out.push('…');
            }
        }
        out.push('\n');
    };

    append_row(&mut out, headers);

    // for _ in 0..term_w {
    //     out.push('─');
    // }
    // out.push('\n');

    for row in rows {
        let cells: Vec<&str> = row.iter().map(|s| s.as_str()).collect();
        append_row(&mut out, &cells);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::format_table;

    #[test]
    fn separator_spans_full_width() {
        let out = format_table(
            &["SLUG", "TITLE"],
            &[vec!["my-preset".into(), "My Preset".into()]],
            40,
        );
        let sep = out.lines().nth(1).unwrap();
        assert_eq!(sep.chars().count(), 40);
    }

    #[test]
    fn two_col_basic_layout() {
        let out = format_table(
            &["SLUG", "TITLE"],
            &[
                vec!["my-preset".into(), "My Preset".into()],
                vec!["another".into(), "Another One".into()],
            ],
            60,
        );
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 4);
        assert!(lines[0].starts_with("SLUG"));
        assert!(lines[2].starts_with("my-preset"));
        assert!(lines[3].starts_with("another  "));
    }

    #[test]
    fn three_col_layout() {
        let out = format_table(
            &["ID", "NAME", "PARAMS"],
            &[vec!["crop".into(), "Crop".into(), "start=0 (time)".into()]],
            60,
        );
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains("ID"));
        assert!(lines[0].contains("NAME"));
        assert!(lines[0].contains("PARAMS"));
    }

    #[test]
    fn last_col_truncates_with_ellipsis() {
        let long_val = "a".repeat(100);
        let out = format_table(
            &["ID", "VALUE"],
            &[vec!["x".into(), long_val]],
            20,
        );
        let row = out.lines().nth(2).unwrap();
        assert!(row.contains('…'));
        let last_cell_start = row.find("  ").unwrap() + 2;
        let last_cell = &row[last_cell_start..];
        assert!(last_cell.chars().count() <= 17);
    }

    #[test]
    fn detail_key_width_computed_across_sections() {
        use super::DetailItem::{Field, Section};
        let items = vec![
            Field("slug", "my-clip".into()),
            Section("file"),
            Field("path", "/music/x.flac".into()),
        ];
        let key_w = items
            .iter()
            .filter_map(|i| if let super::DetailItem::Field(k, _) = i { Some(k.len()) } else { None })
            .max()
            .unwrap_or(0);
        assert_eq!(key_w, 4); // "slug" and "path" are both 4 chars
    }

    #[test]
    fn last_col_fits_without_truncation() {
        let out = format_table(
            &["ID", "VALUE"],
            &[vec!["x".into(), "short".into()]],
            40,
        );
        let row = out.lines().nth(2).unwrap();
        assert!(!row.contains('…'));
        assert!(row.contains("short"));
    }

    #[test]
    fn col_width_driven_by_widest_row() {
        let out = format_table(
            &["ID", "VALUE"],
            &[
                vec!["short".into(), "v1".into()],
                vec!["much-longer-slug".into(), "v2".into()],
            ],
            60,
        );
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines[2].starts_with("short            "));
    }
}
