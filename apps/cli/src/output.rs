use serde::Serialize;

pub fn print_json<T: Serialize>(value: &T) {
    println!("{}", serde_json::to_string_pretty(value).unwrap());
}

pub fn print_table(headers: (&str, &str), rows: Vec<(String, String)>) {
    let col1_w = rows
        .iter()
        .map(|(l, _)| l.len())
        .max()
        .unwrap_or(0)
        .max(headers.0.len());

    println!("{:<col1_w$}  {}", headers.0, headers.1);
    println!("{}", "─".repeat(col1_w + 2 + headers.1.len().min(60)));
    for (left, right) in rows {
        println!("{left:<col1_w$}  {right}");
    }
}

pub fn print_table_3col(
    headers: (&str, &str, &str),
    rows: Vec<(String, String, String)>,
) {
    let c1 = rows.iter().map(|(a, _, _)| a.len()).max().unwrap_or(0).max(headers.0.len());
    let c2 = rows.iter().map(|(_, b, _)| b.len()).max().unwrap_or(0).max(headers.1.len());
    println!("{:<c1$}  {:<c2$}  {}", headers.0, headers.1, headers.2);
    println!("{}", "─".repeat(c1 + c2 + 4 + headers.2.len().min(60)));
    for (a, b, c) in rows {
        println!("{a:<c1$}  {b:<c2$}  {c}");
    }
}

pub fn print_detail(pairs: Vec<(&str, String)>) {
    let key_w = pairs.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
    for (key, val) in pairs {
        println!("{key:>key_w$}: {val}");
    }
}
