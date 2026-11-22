//! The window's arguments: the file to open (`Abrir com` and the command line pass it),
//! `--control PORT` and the automation roots that confine what its control channel may touch.

use std::path::PathBuf;

use craft_mcp::automation::RootFlags;

/// What the window was started with.
#[derive(Debug, Default)]
pub struct Launch {
    /// The file to show first, if any.
    pub file: Option<PathBuf>,
    /// The loopback control channel's port (`--control PORT`).
    pub control: Option<u16>,
    /// The folders the control channel's commands may read and write.
    pub roots: RootFlags,
}

/// Parse the window's arguments. The root flags are the suite's (they may come before or after
/// `--control`, and as `--flag=DIR`), so one client configuration works in every Craft app.
pub fn parse(args: &[String]) -> Result<Launch, String> {
    let (roots, rest) = RootFlags::take(args)?;
    let mut file = None;
    let mut control = None;
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--control" => control = Some(port(it.next().ok_or("--control needs a port")?)?),
            other if other.starts_with("--control=") => control = Some(port(other.trim_start_matches("--control="))?),
            other if other.starts_with("--") => return Err(format!("unknown option `{other}`")),
            other if file.is_none() => file = Some(PathBuf::from(other)),
            // A second file: the window shows one at a time; the first is what was asked for.
            _ => {}
        }
    }
    if control.is_none() && !roots.is_empty() {
        return Err("--automation-read-root and --automation-write-root confine the control channel: start the app with `--control PORT`".into());
    }
    Ok(Launch { file, control, roots })
}

fn port(raw: &str) -> Result<u16, String> {
    let port: u16 = raw.parse().map_err(|_| format!("--control {raw}: a port is 1 to 65535"))?;
    if port == 0 {
        return Err(format!("--control {raw}: a port is 1 to 65535"));
    }
    Ok(port)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn a_plain_path_is_the_file_to_show() {
        let launch = parse(&args(&["C:/fotos/a.png"])).expect("a file");
        assert_eq!(launch.file, Some(PathBuf::from("C:/fotos/a.png")));
        assert!(launch.control.is_none());
        assert!(parse(&args(&[])).is_ok(), "no arguments at all opens the empty window");
    }

    #[test]
    fn control_takes_a_port_and_may_come_before_the_file() {
        let launch = parse(&args(&["--control", "7979", "a.png"])).expect("a control port");
        assert_eq!(launch.control, Some(7979));
        assert_eq!(launch.file, Some(PathBuf::from("a.png")));
        assert_eq!(parse(&args(&["--control=7980"])).expect("inline").control, Some(7980));
        for bad in ["0", "65536", "abc", "-1"] {
            assert!(parse(&args(&["--control", bad])).is_err(), "{bad}");
        }
        assert!(parse(&args(&["--control"])).is_err());
        assert!(parse(&args(&["--nope"])).is_err());
    }

    #[test]
    fn the_roots_belong_to_the_control_channel() {
        let launch = parse(&args(&["--control", "7979", "--automation-read-root", "in"])).expect("roots with control");
        assert_eq!(launch.roots.read, ["in"]);
        let e = parse(&args(&["--automation-read-root", "in"])).unwrap_err();
        assert!(e.contains("confine the control channel") && e.contains("--control"), "{e}");
    }
}
