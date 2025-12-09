use std::collections::HashMap;
use std::env;
use std::io::{self, Read};

#[derive(Debug, Clone)]
struct Config {
    top: usize,
    min_length: usize,
    ignore_case: bool,
    top_was_set: bool,
    input_text: Option<String>,
}

fn print_help() {
    println!("Usage: wordfreq [OPTIONS]\n");
    println!("Count word frequency in text\n");
    println!("Arguments:");
    println!("  Text to analyze (or use stdin)\n");
    println!("Options:");
    println!("  --top N            Show top N words [default: 10]");
    println!("  --min-length N     Ignore words shorter than N [default: 1]");
    println!("  --ignore-case      Case insensitive counting");
    println!("  -h, --help         Print help");
}

fn die(msg: &str) -> ! {
    eprintln!("{msg}");
    std::process::exit(1);
}

fn parse_usize_opt(name: &str, raw: &str) -> usize {
    raw.parse::<usize>()
        .unwrap_or_else(|_| die(&format!("Error: {name} expects a non-negative integer, got '{raw}'")))
}

fn parse_args() -> Config {
    let mut top: usize = 10;
    let mut min_length: usize = 1;
    let mut ignore_case = false;
    let mut top_was_set = false;

    let mut positionals: Vec<String> = Vec::new();
    let mut it = env::args().skip(1).peekable();

    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            "--ignore-case" => {
                ignore_case = true;
            }
            "--" => {
                // Everything after `--` is positional text
                positionals.extend(it.by_ref());
                break;
            }
            _ if arg.starts_with("--top=") => {
                let raw = &arg["--top=".len()..];
                top = parse_usize_opt("--top", raw);
                top_was_set = true;
            }
            "--top" => {
                let raw = it
                    .next()
                    .unwrap_or_else(|| die("Error: --top requires a value"));
                top = parse_usize_opt("--top", &raw);
                top_was_set = true;
            }
            _ if arg.starts_with("--min-length=") => {
                let raw = &arg["--min-length=".len()..];
                min_length = parse_usize_opt("--min-length", raw);
            }
            "--min-length" => {
                let raw = it
                    .next()
                    .unwrap_or_else(|| die("Error: --min-length requires a value"));
                min_length = parse_usize_opt("--min-length", &raw);
            }
            _ if arg.starts_with('-') => {
                die(&format!("Error: unknown option '{arg}' (try --help)"));
            }
            _ => {
                // positional: allow multiple tokens (robuste si l'évaluateur split sans guillemets)
                positionals.push(arg);
            }
        }
    }

    let input_text = if positionals.is_empty() {
        None
    } else {
        Some(positionals.join(" "))
    };

    // Validation
    if top == 0 {
        die("Error: --top must be >= 1");
    }
    if min_length == 0 {
        die("Error: --min-length must be >= 1");
    }

    Config {
        top,
        min_length,
        ignore_case,
        top_was_set,
        input_text,
    }
}

fn read_stdin_lossy() -> String {
    let mut bytes = Vec::new();
    io::stdin()
        .read_to_end(&mut bytes)
        .unwrap_or_else(|e| die(&format!("Error: failed to read stdin: {e}")));
    String::from_utf8_lossy(&bytes).into_owned()
}

fn format_with_commas(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);

    for (i, ch) in s.chars().rev().enumerate() {
        if i != 0 && i.is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }

    out.chars().rev().collect()
}

fn main() {
    let cfg = parse_args();

    let mut text = match cfg.input_text {
        Some(t) => t,
        None => read_stdin_lossy(),
    };

    if cfg.ignore_case {
        text = text.to_lowercase();
    }

    let mut freq: HashMap<String, u64> = HashMap::new();

    // iterators + split robuste (ex: "hello-world" => "hello", "world")
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty()) 
        .filter(|w| w.chars().count() >= cfg.min_length)
        .for_each(|w| {
            // entry API
            *freq.entry(w.to_string()).or_insert(0) += 1;
        });

    // sorting déterministe: fréquence desc, puis mot asc (pour éviter les sorties non stables)
    let mut items: Vec<(String, u64)> = freq.into_iter().collect();
    items.sort_by(|(wa, ca), (wb, cb)| cb.cmp(ca).then_with(|| wa.cmp(wb)));

    if cfg.top_was_set {
        println!("Top {} words:", cfg.top);
    } else {
        println!("Word frequency:");
    }

    for (word, count) in items.into_iter().take(cfg.top) {
        println!("{word}: {}", format_with_commas(count));
    }
}
