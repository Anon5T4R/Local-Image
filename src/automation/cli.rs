//! The command line of Local Image: `mcp` (the MCP server on stdio), `run` (a script of commands,
//! headless), `commands` (the catalogue) and the usage. Anything else is the window, as always.
//!
//! The plumbing is the suite's `craft-mcp`: the server, the script steps with `"$N"` references and
//! the `--automation-read-root`/`--automation-write-root` flags. The commands are Local Image's
//! ([`super::catalogue`]), so the CLI, the MCP server and the palette reach the same ids.

use std::io::Write;
use std::path::Path;

use craft_mcp::automation::RootFlags;
use craft_mcp::remote::Remote;
use serde_json::{Value, json};

use super::catalogue;
use super::mcp::{Automation, Backend};
use super::session::Session;

/// How the command line is used.
pub const USAGE: &str = "\
local-image — Local Image from the command line

USAGE:
  local-image [FILE…]                          open the window (files go to the viewer, as `Abrir com`)
  local-image --control PORT [--automation-read-root DIR]... [--automation-write-root DIR]...
                                               open the window with a loopback control channel on PORT
                                               (127.0.0.1), for `mcp --connect PORT`
  local-image mcp [--connect PORT|HOST:PORT | --headless] [--automation-read-root DIR]... [--automation-write-root DIR]...
                                               run the MCP server on stdio. Default: drive a running
                                               app at 127.0.0.1:7979, falling back to a headless
                                               session. --connect takes PORT (loopback) or HOST:PORT;
                                               a host that isn't loopback is refused. The root flags
                                               confine the files the commands read and write (a kind
                                               left out grants none of its access) and imply a
                                               headless session: a running app enforces its own.
  local-image run [--in FILE] [--cmd ID [--params JSON] | --script FILE|-]...
                                               headless batch: open FILE (or its folder), run the
                                               commands in order and print one JSON result per step.
                                               --script (or --commands) reads a JSON array of
                                               {\"command\": ID, \"params\": {…}} objects, or one per
                                               line; a parameter that is exactly \"$N.path\" takes
                                               that value from step N's result.
  local-image commands [--search TEXT]         print the command catalogue as JSON; --search keeps
                                               the ids, labels or menus containing TEXT.
  local-image --help | --version

Every command the palette has is reachable by id here and through MCP: see `local-image commands`.
";

/// Run the command line when `args` ask for it; `None` means the window (the default).
pub fn run(args: &[String]) -> Option<i32> {
    let outcome = match args.first().map(String::as_str) {
        Some("mcp") => mcp(&args[1..]),
        Some("run") => batch(&args[1..]),
        Some("commands") => commands(&args[1..]),
        Some("-h" | "--help" | "help") => help(),
        Some("-V" | "--version") => {
            let mut out = output();
            let _ = writeln!(out, "local-image {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => return None,
    };
    Some(match outcome {
        Ok(()) => 0,
        Err(e) => {
            let mut out = output();
            let _ = writeln!(out, "local-image: {e}");
            1
        }
    })
}

fn help() -> Result<(), String> {
    let mut out = output();
    let _ = write!(out, "{USAGE}");
    Ok(())
}

/// `mcp [--connect …|--headless] [roots…]`: the MCP server on stdio.
fn mcp(args: &[String]) -> Result<(), String> {
    let (flags, rest) = RootFlags::take(args)?;
    let mut connect: Option<String> = None;
    let mut headless = false;
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--connect" => connect = Some(craft_mcp::remote::normalize_addr(it.next().ok_or("--connect needs an address")?)?),
            other if other.starts_with("--connect=") => connect = Some(craft_mcp::remote::normalize_addr(other.trim_start_matches("--connect="))?),
            "--headless" => headless = true,
            other => return Err(format!("unknown mcp option `{other}`")),
        }
    }
    let roots = flags.roots()?;
    if headless && connect.is_some() {
        return Err("use either --connect or --headless".into());
    }
    if roots.is_some() && connect.is_some() {
        return Err("--automation-read-root and --automation-write-root confine the headless server; a running app enforces its own roots: start it with `local-image --control PORT --automation-read-root DIR`".into());
    }
    let backend: Box<dyn Backend> = if let Some(roots) = roots {
        Box::new(Session::new(Some(roots)))
    } else if headless {
        Box::new(Session::new(None))
    } else if let Some(addr) = connect {
        // An explicit address fails loudly: the agent asked for that app.
        Box::new(Remote::connect(&addr).map_err(|e| format!("cannot connect to {addr}: {e} (start the app with `local-image --control PORT`)"))?)
    } else {
        match Remote::connect(craft_mcp::remote::DEFAULT_ADDR) {
            Ok(remote) => Box::new(remote),
            Err(_) => Box::new(Session::new(None)),
        }
    };
    let mut out = std::io::stderr();
    let _ = writeln!(out, "local-image: MCP server on stdio ({})", backend.describe());
    let mut server = craft_mcp::Server::new(Automation::new(backend));
    server.serve(std::io::stdin().lock(), std::io::stdout().lock()).map_err(|e| e.to_string())
}

/// `run [--in FILE] [--cmd ID [--params JSON] | --script FILE|-]…`: a headless script of commands.
fn batch(args: &[String]) -> Result<(), String> {
    let (flags, rest) = RootFlags::take(args)?;
    let mut session = Session::new(flags.roots()?);
    let mut input: Option<String> = None;
    let mut steps: Vec<(String, Value)> = vec![];
    // The step of the `--cmd` just given, which the next `--params` fills.
    let mut params_for: Option<usize> = None;
    let mut stdin_read = false;
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--in" | "-i" => input = Some(it.next().cloned().ok_or("--in needs a file")?),
            "--cmd" | "-c" => {
                steps.push((it.next().cloned().ok_or("--cmd needs a command id")?, json!({})));
                params_for = Some(steps.len() - 1);
            }
            "--params" | "-p" => {
                let raw = it.next().cloned().ok_or("--params needs JSON")?;
                let params: Value = serde_json::from_str(&raw).map_err(|e| format!("--params {raw}: {e}"))?;
                if !params.is_object() {
                    return Err(format!("--params must be a JSON object, got {raw}"));
                }
                match params_for.and_then(|i| steps.get_mut(i)) {
                    Some((_, slot)) => *slot = params,
                    None => return Err("--params must follow a --cmd".into()),
                }
            }
            "--script" | "--commands" => {
                let flag = arg.clone();
                let source = it.next().cloned().ok_or(format!("{flag} needs a file, or - for standard input"))?;
                if source == "-" && std::mem::replace(&mut stdin_read, true) {
                    return Err(format!("{flag} - is given twice: standard input is read once"));
                }
                let text = craft_mcp::script::read_source(&source, craft_mcp::script::MAX_SOURCE_BYTES).map_err(|e| format!("{flag} {e}"))?;
                let parsed = craft_mcp::script::parse_commands(&format!("{flag} {source}"), &text, steps.len())?;
                steps.extend(parsed.into_iter().map(|c| (c.id, c.params)));
                params_for = None;
            }
            other => return Err(format!("unknown run option `{other}`")),
        }
    }
    let mut out = output();
    let mut printed = |value: &Value| {
        let _ = writeln!(out, "{value}");
    };
    if let Some(path) = &input {
        let opened = if Path::new(path).is_dir() {
            session.call("command.execute", &json!({"command": "file.open_folder", "params": {"path": path}}))
        } else {
            session.call("command.execute", &json!({"command": "file.open", "params": {"path": path}}))
        }
        .map_err(|e| format!("open {path}: {e}"))?;
        printed(&json!({"step": "open", "path": path, "result": opened}));
    }
    let mut results: Vec<Value> = vec![];
    for (n, (id, params)) in steps.into_iter().enumerate() {
        let params = craft_mcp::script::resolve(&params, &results).map_err(|e| format!("step {n} ({id}): {e}"))?;
        let result = session.call("command.execute", &json!({"command": id, "params": params})).map_err(|e| format!("step {n} ({id}): {e}"))?;
        printed(&json!({"step": "cmd", "command": id, "result": result}));
        results.push(result);
    }
    Ok(())
}

/// `commands [--search TEXT]`: the catalogue as JSON.
fn commands(args: &[String]) -> Result<(), String> {
    let mut search: Option<String> = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--search" => search = Some(it.next().cloned().ok_or("--search needs text")?),
            other => return Err(format!("unknown commands option `{other}`")),
        }
    }
    let list = Value::Array(catalogue::search(search.as_deref(), false));
    let mut out = output();
    let _ = writeln!(out, "{}", serde_json::to_string_pretty(&list).unwrap_or_default());
    Ok(())
}

/// Where the command line writes: standard output when it was given to us (a pipe, a file, the
/// MCP client), else the console the shell that started us owns — the Windows build is a GUI
/// binary, which would otherwise write nowhere.
#[cfg(windows)]
fn output() -> Box<dyn Write> {
    use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_OUTPUT_HANDLE, SetConsoleOutputCP};
    // SAFETY: console calls of this process only; a failed attach leaves the Rust stdout.
    let inherited = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) }.is_ok();
    if !inherited {
        unsafe {
            let _ = AttachConsole(ATTACH_PARENT_PROCESS);
            let _ = SetConsoleOutputCP(65001);
        }
    }
    if !inherited && let Ok(file) = std::fs::OpenOptions::new().write(true).open("CONOUT$") {
        return Box::new(file);
    }
    Box::new(std::io::stdout())
}

#[cfg(not(windows))]
fn output() -> Box<dyn Write> {
    Box::new(std::io::stdout())
}
