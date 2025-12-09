use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "hello",
    about = "Rusty Hello - CLI arguments et ownership",
    disable_help_subcommand = true
)]
struct Args {
    /// Name to greet
    #[arg(value_name = "NAME", default_value = "World")]
    name: String,

    /// Convert to uppercase
    #[arg(long)]
    upper: bool,

    /// Repeat greeting N times
    #[arg(
        long,
        value_name = "N",
        default_value_t = 1,
        value_parser = clap::value_parser!(u32).range(1..)
    )]
    repeat: u32,
}

fn main() {
    let args = Args::parse();

    let mut greeting = format!("Hello, {}!", args.name);

    // L'énoncé montre un output entièrement en majuscules : "HELLO, BOB!"
    if args.upper {
        greeting = greeting.to_uppercase();
    }

    for _ in 0..args.repeat {
        println!("{greeting}");
    }
}
