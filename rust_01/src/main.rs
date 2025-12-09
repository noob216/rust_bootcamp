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

fn usage_error(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(2);
}

fn runtime_error(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(1);
}

fn parse_usize_opt(flag: &str, raw: &str) -> usize {
    raw.parse::<usize>().unwrap_or_else(|_| {
        usage_error(&format!(
            "{flag} expects a non-negative integer, got '{raw}'"
        ))
    })
}

// On garde quotes/apostrophes comme partie du token pour passer le test quotes.
// Tout le reste de la ponctuation reste séparateur (hyphen, virgules, etc.)
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '\'' | '"' | '’' | '“' | '”')
}

// min-length doit compter les caractères “utiles” (alphanum), pas les quotes
fn core_len(token: &str) -> usize {
    token.chars().filter(|c| c.is_alphanumeric()).count()
}

fn read_stdin_lossy() -> String {
    let mut bytes = Vec::new();
    io::stdin()
        .read_to_end(&mut bytes)
        .unwrap_or_else(|e| runtime_error(&format!("failed to read stdin: {e}")));
    String::from_utf8_lossy(&bytes).into_owned()
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
                    .unwrap_or_else(|| usage_error("--top requires a value"));
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
                    .unwrap_or_else(|| usage_error("--min-length requires a value"));
                min_length = parse_usize_opt("--min-length", &raw);
            }
            _ if arg.starts_with('-') => {
                usage_error(&format!("unknown option '{arg}' (try --help)"));
            }
            _ => positionals.push(arg),
        }
    }

    let input_text = if positionals.is_empty() {
        None
    } else {
        Some(positionals.join(" "))
    };

    Config {
        top,
        min_length,
        ignore_case,
        top_was_set,
        input_text,
    }
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

    text.split(|c: char| !is_word_char(c))
        .filter(|w| !w.is_empty())
        .filter(|w| core_len(w) >= cfg.min_length)
        .for_each(|w| {
            *freq.entry(w.to_string()).or_insert(0) += 1;
        });

    let mut items: Vec<(String, u64)> = freq.into_iter().collect();
    items.sort_by(|(wa, ca), (wb, cb)| cb.cmp(ca).then_with(|| wa.cmp(wb)));

    if cfg.top_was_set {
        println!("Top {} words:", cfg.top);
    } else {
        println!("Word frequency:");
    }

    for (word, count) in items.into_iter().take(cfg.top) {
        println!("{word}: {count}");
    }
}
