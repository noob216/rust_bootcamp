use clap::Parser;
use rand::RngCore;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, VecDeque};
use std::fs;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};

const MAX_SIDE: usize = 512;
const MAX_CELLS: usize = MAX_SIDE * MAX_SIDE;

#[derive(Parser, Debug)]
#[command(
    name = "hexpath",
    about = "Find min/max cost paths in hexadecimal grid",
    disable_help_subcommand = true
)]
struct Cli {
    /// Generate random map (e.g. 8x4, 10x10)
    #[arg(long = "generate", value_name = "WxH")]
    generate: Option<String>,

    /// Save generated map to file
    #[arg(long = "output", value_name = "FILE")]
    output: Option<PathBuf>,

    /// Show colored map
    #[arg(long = "visualize")]
    visualize: bool,

    /// Show both min and max paths
    #[arg(long = "both")]
    both: bool,

    /// Animate pathfinding
    #[arg(long = "animate")]
    animate: bool,

    /// Map file (hex values, space separated)
    map_file: Option<PathBuf>,
}

fn main() {
    let cli = Cli::parse();

    let code = match entry(cli) {
        Ok(()) => 0,
        Err(Exit::Cli(msg)) => {
            eprintln!("error: {msg}");
            2
        }
        Err(Exit::Runtime(msg)) => {
            eprintln!("error: {msg}");
            1
        }
    };

    std::process::exit(code);
}

enum Exit {
    Cli(String),
    Runtime(String),
}

/*CLI / ENTRY*/

fn entry(cli: Cli) -> Result<(), Exit> {
    // Validation des combinaisons d’options
    if cli.generate.is_some() && cli.map_file.is_some() {
        return Err(Exit::Cli(
            "cannot use MAP_FILE together with --generate".to_string(),
        ));
    }
    if cli.generate.is_none() && cli.map_file.is_none() {
        return Err(Exit::Cli(
            "missing input: provide MAP_FILE or use --generate WxH".to_string(),
        ));
    }
    if cli.output.is_some() && cli.generate.is_none() {
        return Err(Exit::Cli("--output requires --generate WxH".to_string()));
    }

    // Génération map aléatoire
    if let Some(spec) = cli.generate.as_deref() {
        let (w, h) = parse_wh(spec).map_err(Exit::Cli)?;
        let grid = generate_grid(w, h);

        if let Some(path) = cli.output.as_deref() {
            write_grid_file(path, &grid).map_err(Exit::Runtime)?;
            // Chaîne attendue par le runner
            println!("Map saved to: {}", path.display());
        } else {
            println!("{}", format_grid(&grid));
        }

        // Si on demande en plus une analyse/visualisation sur la map générée
        if cli.visualize || cli.both || cli.animate {
            analyze_and_print(&grid, cli.visualize, cli.both, cli.animate)?;
        }
        return Ok(());
    }

    // Analyse fichier existant
    let path = cli.map_file.as_ref().expect("validated");
    let content = fs::read_to_string(path)
        .map_err(|e| Exit::Runtime(format!("failed to read '{}': {e}", path.display())))?;
    let grid = parse_grid_text(&content).map_err(Exit::Cli)?;

    analyze_and_print(&grid, cli.visualize, cli.both, cli.animate)
}

fn analyze_and_print(grid: &Grid, visualize: bool, both: bool, animate: bool) -> Result<(), Exit> {
    validate_grid(grid).map_err(Exit::Cli)?;

    println!("Analyzing hexadecimal grid...");
    println!("Grid size: {}x{}", grid.w, grid.h);
    println!("Start: (0,0) = 0x{:02X}", grid.at(0, 0).unwrap_or(0));
    println!(
        "End: ({},{}) = 0x{:02X}",
        grid.w - 1,
        grid.h - 1,
        grid.at(grid.w - 1, grid.h - 1).unwrap_or(0)
    );
    println!();

    // Chemin de coût minimal (Dijkstra)
    let (min_cost, min_path) = dijkstra_min_cost(grid).map_err(Exit::Runtime)?;

    println!("MINIMUM COST PATH:");
    print_path_report(grid, min_cost, &min_path);

    // Chemin de coût maximal parmi les chemins à nb de pas minimal
    let max_res = if both {
        max_cost_among_shortest_paths(grid)
    } else {
        None
    };

    if both {
        println!();
        println!("MAXIMUM COST PATH:");
        if let Some((max_cost, ref max_path)) = max_res {
            print_path_report(grid, max_cost, max_path);
        } else {
            println!("No path found.");
        }
    }

    if visualize {
        println!();
        let max_path_ref = max_res.as_ref().map(|(_, p)| p.as_slice());
        print_visualization(grid, &min_path, max_path_ref);
    }

    if animate {
        println!();
        run_light_animation(grid);
    }

    Ok(())
}

/*GRID / PARSING*/

#[derive(Clone, Debug)]
struct Grid {
    w: usize,
    h: usize,
    cells: Vec<u8>,
}

impl Grid {
    fn idx(&self, x: usize, y: usize) -> Option<usize> {
        if x < self.w && y < self.h {
            Some(y * self.w + x)
        } else {
            None
        }
    }

    fn at(&self, x: usize, y: usize) -> Option<u8> {
        self.idx(x, y).and_then(|i| self.cells.get(i).copied())
    }
}

fn parse_wh(s: &str) -> Result<(usize, usize), String> {
    let s = s.trim();
    let (w_s, h_s) = s
        .split_once('x')
        .or_else(|| s.split_once('X'))
        .ok_or_else(|| format!("invalid size '{s}' (expected WxH, e.g. 10x10)"))?;
    let w: usize = w_s
        .trim()
        .parse()
        .map_err(|_| format!("invalid width in '{s}'"))?;
    let h: usize = h_s
        .trim()
        .parse()
        .map_err(|_| format!("invalid height in '{s}'"))?;
    if w == 0 || h == 0 {
        return Err("width and height must be > 0".to_string());
    }
    if w > MAX_SIDE || h > MAX_SIDE || w * h > MAX_CELLS {
        return Err("grid too large".to_string());
    }
    Ok((w, h))
}

fn generate_grid(w: usize, h: usize) -> Grid {
    let mut rng = rand::thread_rng();
    let mut cells = Vec::with_capacity(w * h);

    for _ in 0..(w * h) {
        let val = (rng.next_u32() & 0xFF) as u8;
        cells.push(val);
    }

    // Contraintes : 00 (top-left), FF (bottom-right)
    if let Some(first) = cells.first_mut() {
        *first = 0x00;
    }
    if let Some(last) = cells.last_mut() {
        *last = 0xFF;
    }
    Grid { w, h, cells }
}

fn write_grid_file(path: &Path, grid: &Grid) -> Result<(), String> {
    let mut out = String::new();
    for y in 0..grid.h {
        for x in 0..grid.w {
            if x > 0 {
                out.push(' ');
            }
            let v = grid.at(x, y).unwrap_or(0);
            out.push_str(&format!("{v:02X}"));
        }
        out.push('\n');
    }
    fs::write(path, out).map_err(|e| format!("failed to write '{}': {e}", path.display()))
}

fn format_grid(grid: &Grid) -> String {
    let mut out = String::new();
    for y in 0..grid.h {
        for x in 0..grid.w {
            if x > 0 {
                out.push(' ');
            }
            let v = grid.at(x, y).unwrap_or(0);
            out.push_str(&format!("{v:02X}"));
        }
        if y + 1 < grid.h {
            out.push('\n');
        }
    }
    out
}

fn parse_grid_text(content: &str) -> Result<Grid, String> {
    let mut rows: Vec<Vec<u8>> = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let mut row = Vec::new();
        for tok in line.split_whitespace() {
            let t0 = tok.trim().trim_end_matches(',').trim_end_matches(';');
            let t = t0
                .strip_prefix("0x")
                .or_else(|| t0.strip_prefix("0X"))
                .unwrap_or(t0);

            if t.is_empty() {
                return Err("empty hex token".to_string());
            }
            if t.len() > 2 {
                return Err(format!("invalid hex token '{tok}' (expected 00-FF)"));
            }
            let v = u8::from_str_radix(t, 16)
                .map_err(|_| format!("invalid hex token '{tok}' (expected 00-FF)"))?;
            row.push(v);
        }
        if !row.is_empty() {
            rows.push(row);
        }
    }

    if rows.is_empty() {
        return Err("empty map".to_string());
    }

    let w = rows[0].len();
    if w == 0 {
        return Err("invalid map width".to_string());
    }
    if w > MAX_SIDE {
        return Err("grid too wide".to_string());
    }
    for (i, r) in rows.iter().enumerate() {
        if r.len() != w {
            return Err(format!("non-rectangular map at row {i}"));
        }
    }

    let h = rows.len();
    if h > MAX_SIDE || w * h > MAX_CELLS {
        return Err("grid too large".to_string());
    }

    let mut cells = Vec::with_capacity(w * h);
    for r in rows {
        cells.extend(r);
    }

    Ok(Grid { w, h, cells })
}

fn validate_grid(grid: &Grid) -> Result<(), String> {
    if grid.w == 0 || grid.h == 0 {
        return Err("invalid grid dimensions".to_string());
    }
    if grid.cells.len() != grid.w * grid.h {
        return Err("invalid grid storage".to_string());
    }
    if grid.at(0, 0) != Some(0x00) {
        return Err("start (top-left) must be 00".to_string());
    }
    if grid.at(grid.w - 1, grid.h - 1) != Some(0xFF) {
        return Err("end (bottom-right) must be FF".to_string());
    }
    Ok(())
}

/*MIN COST (Dijkstra)*/

#[derive(Copy, Clone, Eq, PartialEq)]
struct State {
    cost: u64,
    idx: usize,
}

impl Ord for State {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .cmp(&self.cost)
            .then_with(|| other.idx.cmp(&self.idx))
    }
}

impl PartialOrd for State {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn dijkstra_min_cost(grid: &Grid) -> Result<(u64, Vec<(usize, usize)>), String> {
    let n = grid.w * grid.h;
    let start = 0usize;
    let goal = n - 1;

    let mut dist = vec![u64::MAX; n];
    let mut prev: Vec<Option<usize>> = vec![None; n];
    let mut heap = BinaryHeap::new();

    dist[start] = 0;
    heap.push(State {
        cost: 0,
        idx: start,
    });

    while let Some(State { cost, idx }) = heap.pop() {
        if cost != dist[idx] {
            continue;
        }
        if idx == goal {
            break;
        }

        let x = idx % grid.w;
        let y = idx / grid.w;

        for (nx, ny) in neighbors4(x, y, grid.w, grid.h) {
            let nidx = ny * grid.w + nx;
            let w = grid.at(nx, ny).unwrap_or(0) as u64;
            let next = cost.saturating_add(w);
            if next < dist[nidx] {
                dist[nidx] = next;
                prev[nidx] = Some(idx);
                heap.push(State {
                    cost: next,
                    idx: nidx,
                });
            }
        }
    }

    if dist[goal] == u64::MAX {
        return Err("no path found".to_string());
    }

    let path = reconstruct_path(prev, grid.w, goal);
    Ok((dist[goal], path))
}

/*MAX COST parmi les chemins à nombre de pas minimal*/

fn max_cost_among_shortest_paths(grid: &Grid) -> Option<(u64, Vec<(usize, usize)>)> {
    let n = grid.w * grid.h;
    let start = 0usize;
    let goal = n - 1;

    // BFS pour distance en nombre de pas
    let mut step = vec![i32::MAX; n];
    let mut q = VecDeque::new();
    step[start] = 0;
    q.push_back(start);

    while let Some(idx) = q.pop_front() {
        let x = idx % grid.w;
        let y = idx / grid.w;
        let d = step[idx];

        for (nx, ny) in neighbors4(x, y, grid.w, grid.h) {
            let nidx = ny * grid.w + nx;
            if step[nidx] == i32::MAX {
                step[nidx] = d + 1;
                q.push_back(nidx);
            }
        }
    }

    let goal_d = step[goal];
    if goal_d == i32::MAX {
        return None;
    }

    // DP pour coût max sur le DAG des distances
    let mut best = vec![i64::MIN; n];
    let mut prev: Vec<Option<usize>> = vec![None; n];
    best[start] = 0;

    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); (goal_d as usize) + 1];
    for (i, &d) in step.iter().enumerate() {
        if d != i32::MAX {
            layers[d as usize].push(i);
        }
    }

    let limit = goal_d as usize;
    for (d, layer) in layers.iter().enumerate().take(limit) {
        for &idx in layer {
            if best[idx] == i64::MIN {
                continue;
            }
            let x = idx % grid.w;
            let y = idx / grid.w;
            for (nx, ny) in neighbors4(x, y, grid.w, grid.h) {
                let nidx = ny * grid.w + nx;
                if step[nidx] == (d as i32) + 1 {
                    let add = grid.at(nx, ny).unwrap_or(0) as i64;
                    let cand = best[idx].saturating_add(add);
                    if cand > best[nidx] {
                        best[nidx] = cand;
                        prev[nidx] = Some(idx);
                    }
                }
            }
        }
    }

    if best[goal] == i64::MIN {
        return None;
    }

    let path = reconstruct_path(prev, grid.w, goal);
    Some((best[goal] as u64, path))
}

/*Reporting / UI*/

fn print_path_report(grid: &Grid, total: u64, path: &[(usize, usize)]) {
    println!("Total cost: 0x{:X} ({} decimal)", total, total);
    println!("Path length: {} steps", path.len());
    print!("Path: ");
    for (i, (x, y)) in path.iter().enumerate() {
        if i > 0 {
            print!("->");
        }
        print!("({x},{y})");
    }
    println!();
    println!();
    println!("Step-by-step costs:");
    println!("Start 0x00 (0,0)");
    let mut acc = 0u64;
    for &(x, y) in path.iter().skip(1) {
        let v = grid.at(x, y).unwrap_or(0) as u64;
        acc = acc.saturating_add(v);
        println!("+ 0x{:02X} ({},{}) -> {}", v as u8, x, y, acc);
    }
    println!("Total: 0x{:X} ({})", total, total);
}

fn print_visualization(
    grid: &Grid,
    min_path: &[(usize, usize)],
    max_path: Option<&[(usize, usize)]>,
) {
    let use_color = io::stdout().is_terminal();

    let mut min_mask = vec![false; grid.w * grid.h];
    for &(x, y) in min_path {
        if let Some(i) = grid.idx(x, y) {
            min_mask[i] = true;
        }
    }

    let mut max_mask = vec![false; grid.w * grid.h];
    if let Some(p) = max_path {
        for &(x, y) in p {
            if let Some(i) = grid.idx(x, y) {
                max_mask[i] = true;
            }
        }
    }

    println!("HEX GRID:");
    for y in 0..grid.h {
        for x in 0..grid.w {
            if x > 0 {
                print!(" ");
            }
            let i = grid.idx(x, y).unwrap();
            let v = grid.cells[i];

            if use_color {
                if max_mask[i] {
                    // chemin max en rouge
                    print!("\x1b[31m{:02X}\x1b[0m", v);
                } else if min_mask[i] {
                    // chemin min en blanc
                    print!("\x1b[97m{:02X}\x1b[0m", v);
                } else {
                    let c = rainbow_ansi256(v);
                    print!("\x1b[38;5;{}m{:02X}\x1b[0m", c, v);
                }
            } else {
                print!("{:02X}", v);
            }
        }
        println!();
    }
}

fn rainbow_ansi256(v: u8) -> u8 {
    let t = v as u16;
    let r = ((t * 5) / 255) as u8;
    let g = (((t * 5) / 255 + 2) % 6) as u8;
    let b = (((t * 5) / 255 + 4) % 6) as u8;
    16 + 36 * r + 6 * g + b
}

fn run_light_animation(grid: &Grid) {
    println!("Searching for minimum cost path...");
    let n = grid.w * grid.h;
    let mut seen = vec![false; n];
    let mut q = VecDeque::new();
    seen[0] = true;
    q.push_back(0usize);

    let mut step_no = 0usize;
    while let Some(idx) = q.pop_front() {
        step_no += 1;
        let x = idx % grid.w;
        let y = idx / grid.w;
        println!("Step {}: Exploring ({},{})", step_no, x, y);
        if step_no >= 8 {
            println!("[Animation continues...]");
            break;
        }
        for (nx, ny) in neighbors4(x, y, grid.w, grid.h) {
            let nidx = ny * grid.w + nx;
            if !seen[nidx] {
                seen[nidx] = true;
                q.push_back(nidx);
            }
        }
    }
}

/*util*/

fn neighbors4(x: usize, y: usize, w: usize, h: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::with_capacity(4);
    if y > 0 {
        out.push((x, y - 1));
    }
    if y + 1 < h {
        out.push((x, y + 1));
    }
    if x > 0 {
        out.push((x - 1, y));
    }
    if x + 1 < w {
        out.push((x + 1, y));
    }
    out
}

fn reconstruct_path(prev: Vec<Option<usize>>, w: usize, goal: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut cur = Some(goal);
    while let Some(i) = cur {
        out.push((i % w, i / w));
        cur = prev[i];
    }
    out.reverse();
    out
}
