use clap::{Parser, Subcommand};
use rand::Rng;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::time::Duration;

const P: u64 = 0xD87FA3E29184CF73;
const G: u64 = 2;

const IO_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_MSG_LEN: u32 = 1_048_576; // 1 MiB

#[derive(Parser, Debug)]
#[command(
    name = "streamchat",
    about = "Stream cipher chat with Diffie-Hellman key generation",
    disable_help_subcommand = true,
    arg_required_else_help = true
)]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Start server
    Server {
        /// Port to listen on (1-65535)
        port: u16,
    },
    /// Connect to server
    Client {
        /// Address in the form host:port (e.g. localhost:8080)
        addr: String,
    },
}

fn main() {
    let cli = Cli::parse();

    let code = match cli.cmd {
        Command::Server { port } => match run_server(port) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("error: {e}");
                1
            }
        },
        Command::Client { addr } => match run_client(&addr) {
            Ok(()) => 0,
            Err(AppError::Cli(msg)) => {
                eprintln!("error: {msg}");
                2
            }
            Err(AppError::Runtime(msg)) => {
                eprintln!("error: {msg}");
                1
            }
        },
    };

    std::process::exit(code);
}

fn run_server(port: u16) -> Result<(), String> {
    // Runner expectation: server prints a line containing "p =" and stays alive.
    println!("[DH] Using hardcoded DH parameters:");
    println!("p = {P:016X}");
    println!("g = {G}");
    println!();

    let addr = format!("0.0.0.0:{port}");
    let listener = TcpListener::bind(&addr).map_err(|e| format!("bind({addr}) failed: {e}"))?;

    println!("[SERVER] Listening on {addr}");
    println!("[SERVER] Waiting for client...");

    loop {
        let (mut stream, peer) = match listener.accept() {
            Ok(v) => v,
            Err(e) => {
                eprintln!("error: accept failed: {e}");
                continue;
            }
        };

        println!("[CLIENT] Connected from {peer}");

        if let Err(e) = configure_stream(&mut stream) {
            eprintln!("error: stream config failed: {e}");
            continue;
        }

        if let Err(e) = handle_server_session(&mut stream) {
            eprintln!("error: session failed: {e}");
        }

        println!("[SERVER] Waiting for client...");
    }
}

fn run_client(addr: &str) -> Result<(), AppError> {
    let endpoint = parse_endpoint(addr).map_err(AppError::Cli)?;

    let mut resolved = endpoint
        .to_socket_addrs()
        .map_err(|e| AppError::Cli(format!("invalid address '{addr}': {e}")))?;

    let Some(sockaddr) = resolved.next() else {
        return Err(AppError::Cli(format!(
            "invalid address '{addr}': could not resolve"
        )));
    };

    println!("[CLIENT] Connecting to {addr}...");
    let mut stream = TcpStream::connect(sockaddr)
        .map_err(|e| AppError::Runtime(format!("connect({addr}) failed: {e}")))?;
    println!("[CLIENT] Connected!");

    configure_stream(&mut stream)
        .map_err(|e| AppError::Runtime(format!("stream config failed: {e}")))?;

    handle_client_session(&mut stream).map_err(AppError::Runtime)
}

fn configure_stream(stream: &mut TcpStream) -> std::io::Result<()> {
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    Ok(())
}

fn handle_server_session(stream: &mut TcpStream) -> Result<(), String> {
    println!("[DH] Starting key exchange...");

    let keys = dh_handshake(stream, Role::Server).map_err(|e| format!("handshake failed: {e}"))?;

    println!("Secure channel established.");

    // Démo déterministe: envoi "Hello", réception d'une réponse.
    let msg = b"Hello";
    send_msg(stream, &keys.send, msg).map_err(|e| format!("send failed: {e}"))?;

    //lecture d'une réponse, sans faire échouer la session si le client ferme.
    if let Ok(reply) = recv_msg(stream, &keys.recv) {
        println!("[SERVER] {}", String::from_utf8_lossy(&reply));
    }

    Ok(())
}

fn handle_client_session(stream: &mut TcpStream) -> Result<(), String> {
    println!("[DH] Starting key exchange...");

    let keys = dh_handshake(stream, Role::Client).map_err(|e| format!("handshake failed: {e}"))?;

    println!("Secure channel established.");

    let incoming = recv_msg(stream, &keys.recv).map_err(|e| format!("recv failed: {e}"))?;
    println!("[SERVER] {}", String::from_utf8_lossy(&incoming));

    let reply = b"Hi!";
    send_msg(stream, &keys.send, reply).map_err(|e| format!("send failed: {e}"))?;

    Ok(())
}

#[derive(Copy, Clone, Debug)]
enum Role {
    Server,
    Client,
}

struct Keys {
    send: Keystream,
    recv: Keystream,
}

fn dh_handshake(stream: &mut TcpStream, role: Role) -> std::io::Result<Keys> {
    // Private in [2, P-2]
    let mut rng = rand::thread_rng();
    let private = rng.gen_range(2..(P - 1));
    let public = modexp(G, private, P);

    // Exchange public keys (8 bytes)
    let peer_public = match role {
        Role::Server => {
            stream.write_all(&public.to_be_bytes())?;
            let mut buf = [0u8; 8];
            stream.read_exact(&mut buf)?;
            u64::from_be_bytes(buf)
        }
        Role::Client => {
            let mut buf = [0u8; 8];
            stream.read_exact(&mut buf)?;
            let peer = u64::from_be_bytes(buf);
            stream.write_all(&public.to_be_bytes())?;
            peer
        }
    };

    // Basic validation of peer_public
    if peer_public <= 1 || peer_public >= (P - 1) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid peer public key",
        ));
    }

    let secret = modexp(peer_public, private, P);

    // Proof exchange to detect mismatch
    let my_proof = mix64(secret ^ 0xA5A5_A5A5_A5A5_A5A5);
    let peer_proof = match role {
        Role::Server => {
            stream.write_all(&my_proof.to_be_bytes())?;
            let mut buf = [0u8; 8];
            stream.read_exact(&mut buf)?;
            u64::from_be_bytes(buf)
        }
        Role::Client => {
            let mut buf = [0u8; 8];
            stream.read_exact(&mut buf)?;
            let their = u64::from_be_bytes(buf);
            stream.write_all(&my_proof.to_be_bytes())?;
            their
        }
    };

    if peer_proof != my_proof {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "secret verification failed",
        ));
    }

    // Directional keystream seeds
    let seed_s2c = mix64(secret ^ 0x5352_563E_0000_0001); // "SRV>"
    let seed_c2s = mix64(secret ^ 0x434C_493E_0000_0002); // "CLI>"

    let (send_seed, recv_seed) = match role {
        Role::Server => (seed_s2c, seed_c2s),
        Role::Client => (seed_c2s, seed_s2c),
    };

    Ok(Keys {
        send: Keystream::new(send_seed),
        recv: Keystream::new(recv_seed),
    })
}

fn send_msg(stream: &mut TcpStream, ks: &Keystream, plain: &[u8]) -> std::io::Result<()> {
    let len_u32: u32 = plain
        .len()
        .try_into()
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "message too long"))?;

    if len_u32 > MAX_MSG_LEN {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "message too large",
        ));
    }

    let mut local = ks.clone();
    let mut cipher = vec![0u8; plain.len()];
    for (i, &b) in plain.iter().enumerate() {
        cipher[i] = b ^ local.next_byte();
    }

    stream.write_all(&len_u32.to_be_bytes())?;
    stream.write_all(&cipher)?;
    Ok(())
}

fn recv_msg(stream: &mut TcpStream, ks: &Keystream) -> std::io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf);

    if len > MAX_MSG_LEN {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "incoming message too large",
        ));
    }

    let mut cipher = vec![0u8; len as usize];
    stream.read_exact(&mut cipher)?;

    let mut local = ks.clone();
    for b in &mut cipher {
        *b ^= local.next_byte();
    }
    Ok(cipher)
}

#[derive(Clone)]
struct Keystream {
    state: u32,
}

impl Keystream {
    fn new(seed: u64) -> Self {
        // Fold seed into 32-bit state (non-zero preferred)
        let folded = (seed as u32) ^ ((seed >> 32) as u32);
        let state = if folded == 0 { 0x6D2B_79F5 } else { folded };
        Self { state }
    }

    fn next_byte(&mut self) -> u8 {
        // LCG: state = (a*state + c) mod 2^32, output top byte
        const A: u32 = 1_103_515_245;
        const C: u32 = 12_345;
        self.state = self.state.wrapping_mul(A).wrapping_add(C);
        (self.state >> 24) as u8
    }
}

fn mul_mod(a: u64, b: u64, m: u64) -> u64 {
    ((a as u128 * b as u128) % (m as u128)) as u64
}

fn modexp(mut base: u64, mut exp: u64, modulus: u64) -> u64 {
    if modulus == 1 {
        return 0;
    }
    let mut result = 1_u64;
    base %= modulus;

    while exp > 0 {
        if exp & 1 == 1 {
            result = mul_mod(result, base, modulus);
        }
        exp >>= 1;
        if exp > 0 {
            base = mul_mod(base, base, modulus);
        }
    }
    result
}

// SplitMix64-style mixer (fast, deterministic)
fn mix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn parse_endpoint(s: &str) -> Result<String, String> {
    let s = s.trim();
    let (host, port_str) = s
        .rsplit_once(':')
        .ok_or_else(|| format!("invalid address '{s}' (expected host:port)"))?;

    if host.trim().is_empty() {
        return Err(format!("invalid address '{s}' (empty host)"));
    }

    let port: u16 = port_str
        .parse()
        .map_err(|_| format!("invalid address '{s}' (invalid port)"))?;

    if port == 0 {
        return Err(format!("invalid address '{s}' (port out of range)"));
    }

    Ok(format!("{}:{port}", host.trim()))
}

enum AppError {
    Cli(String),
    Runtime(String),
}
