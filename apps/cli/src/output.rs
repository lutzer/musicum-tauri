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

pub fn print_detail(pairs: Vec<(&str, String)>) {
    let key_w = pairs.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
    for (key, val) in pairs {
        println!("{key:>key_w$}: {val}");
    }
}
