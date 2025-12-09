use clap::Parser;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "hextool",
    about = "Read and write binary files in hexadecimal",
    disable_help_flag = true,
    disable_help_subcommand = true
)]
struct Cli {
    /// Target file
    #[arg(short = 'f', long = "file")]
    file: Option<PathBuf>,

    /// Read mode (display hex)
    #[arg(short = 'r', long = "read", conflicts_with = "write")]
    read: bool,

    /// Write mode (hex string to write)
    #[arg(
        short = 'w',
        long = "write",
        value_name = "HEX",
        conflicts_with = "read"
    )]
    write: Option<String>,

    /// Offset in bytes (decimal or 0x hex)
    #[arg(short = 'o', long = "offset", value_name = "OFFSET", value_parser = parse_u64_dec_or_hex)]
    offset: Option<u64>,

    /// Number of bytes to read
    #[arg(short = 's', long = "size", value_name = "SIZE", value_parser = parse_u64_dec_or_hex)]
    size: Option<u64>,

    /// Print help
    #[arg(short = 'h', long = "help")]
    help: bool,
}

fn print_help() {
    println!("Usage: hextool [OPTIONS]\n");
    println!("Read and write binary files in hexadecimal\n");
    println!("Options:");
    println!("-f, --file   Target file");
    println!("-r, --read   Read mode (display hex)");
    println!("-w, --write  Write mode (hex string to write)");
    println!("-o, --offset Offset in bytes (decimal or 0x hex)");
    println!("-s, --size   Number of bytes to read");
    println!("-h, --help   Print help");
}

fn parse_u64_dec_or_hex(raw: &str) -> Result<u64, String> {
    let s = raw.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        if hex.is_empty() {
            return Err("empty hex value".to_string());
        }
        u64::from_str_radix(hex, 16)
            .map_err(|_| format!("invalid number '{raw}' (expected decimal or 0x hex)"))
    } else {
        if s.is_empty() {
            return Err("empty decimal value".to_string());
        }
        s.parse::<u64>()
            .map_err(|_| format!("invalid number '{raw}' (expected decimal or 0x hex)"))
    }
}

fn is_printable_ascii(b: u8) -> bool {
    (0x20..=0x7e).contains(&b)
}

fn bytes_to_spaced_hex(bytes: &[u8]) -> String {
    let mut out = String::new();
    for (i, b) in bytes.iter().enumerate() {
        if i != 0 {
            out.push(' ');
        }
        out.push_str(&format!("{:02x}", b));
    }
    out
}

fn bytes_to_ascii(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| {
            if is_printable_ascii(b) {
                b as char
            } else {
                '.'
            }
        })
        .collect()
}

fn parse_hex_string_to_bytes(input: &str) -> Result<Vec<u8>, String> {
    let trimmed = input.trim();
    let no_prefix = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);

    let cleaned: Vec<u8> = no_prefix
        .bytes()
        .filter(|b| !b.is_ascii_whitespace() && *b != b'_')
        .collect();

    if cleaned.is_empty() {
        return Err("hex string is empty".to_string());
    }
    if !cleaned.len().is_multiple_of(2) {
        return Err("hex string must have an even number of digits".to_string());
    }

    fn hex_val(b: u8) -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    }

    let mut out = Vec::with_capacity(cleaned.len() / 2);
    for i in (0..cleaned.len()).step_by(2) {
        let hi = hex_val(cleaned[i]).ok_or_else(|| "invalid hex digit".to_string())?;
        let lo = hex_val(cleaned[i + 1]).ok_or_else(|| "invalid hex digit".to_string())?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn die(msg: &str) -> ! {
    eprintln!("Error: {msg}");
    std::process::exit(1);
}

fn main() {
    let cli = Cli::parse();

    if cli.help {
        print_help();
        return;
    }

    let file_path = cli
        .file
        .unwrap_or_else(|| die("--file is required (try --help)"));
    let offset = cli.offset.unwrap_or(0);

    let mode_read = cli.read;
    let mode_write = cli.write.is_some();

    if mode_read == mode_write {
        die("choose exactly one mode: --read or --write (try --help)");
    }

    if mode_read {
        run_read(&file_path, offset, cli.size);
    } else {
        let hex = cli.write.expect("write mode guaranteed");
        run_write(&file_path, offset, &hex);
    }
}

fn run_read(path: &PathBuf, offset: u64, size: Option<u64>) {
    let mut file = std::fs::File::open(path).unwrap_or_else(|e| {
        die(&format!("failed to open file '{:?}': {e}", path));
    });

    let len = file
        .metadata()
        .map(|m| m.len())
        .unwrap_or_else(|e| die(&format!("failed to stat file '{:?}': {e}", path)));

    if offset > len {
        die("invalid offset (past end of file)");
    }

    let available = len - offset;
    let to_read = size.unwrap_or(available).min(available);

    file.seek(SeekFrom::Start(offset))
        .unwrap_or_else(|e| die(&format!("failed to seek: {e}")));

    let mut remaining = to_read;
    let mut base_off = offset;

    while remaining > 0 {
        let chunk_len = remaining.min(16) as usize;
        let mut buf = vec![0u8; chunk_len];

        let mut read_total = 0usize;
        while read_total < chunk_len {
            let n = file
                .read(&mut buf[read_total..])
                .unwrap_or_else(|e| die(&format!("failed to read: {e}")));
            if n == 0 {
                break;
            }
            read_total += n;
        }
        buf.truncate(read_total);

        if buf.is_empty() {
            break;
        }

        let hex_part = bytes_to_spaced_hex(&buf);
        let ascii_part = bytes_to_ascii(&buf);
        println!("{:08x}: {} |{}|", base_off, hex_part, ascii_part);

        base_off += buf.len() as u64;
        remaining -= buf.len() as u64;
    }
}

fn run_write(path: &PathBuf, offset: u64, hex: &str) {
    let bytes =
        parse_hex_string_to_bytes(hex).unwrap_or_else(|e| die(&format!("invalid hex: {e}")));

    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)
        .unwrap_or_else(|e| die(&format!("failed to open file '{:?}': {e}", path)));

    let len = file
        .metadata()
        .map(|m| m.len())
        .unwrap_or_else(|e| die(&format!("failed to stat file '{:?}': {e}", path)));

    // Si offset > EOF, on comble le gap avec des espaces (0x20) pour matcher l’exemple Hello World
    if offset > len {
        file.seek(SeekFrom::End(0))
            .unwrap_or_else(|e| die(&format!("failed to seek: {e}")));

        let mut gap = offset - len;
        let filler = [0x20u8; 8192];
        while gap > 0 {
            let n = (gap as usize).min(filler.len());
            file.write_all(&filler[..n])
                .unwrap_or_else(|e| die(&format!("failed to fill gap: {e}")));
            gap -= n as u64;
        }
    }

    file.seek(SeekFrom::Start(offset))
        .unwrap_or_else(|e| die(&format!("failed to seek: {e}")));
    file.write_all(&bytes)
        .unwrap_or_else(|e| die(&format!("failed to write: {e}")));
    file.flush()
        .unwrap_or_else(|e| die(&format!("failed to flush: {e}")));

    println!("Writing {} bytes at offset 0x{:08x}", bytes.len(), offset);
    println!("Hex: {}", bytes_to_spaced_hex(&bytes));
    println!("ASCII: {}", bytes_to_ascii(&bytes));
    println!("Successfully written");
}
