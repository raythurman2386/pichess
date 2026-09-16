//! Launch-contract argument parsing plus a headless perft flag for
//! engine debugging.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliAction {
    Run,
    Version,
    Help,
    /// `--perft N`: run start-position perft to depth N and print the count.
    Perft(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliError {
    pub argument: String,
}

pub fn parse_cli(args: &[String]) -> Result<CliAction, CliError> {
    match args.get(1).map(String::as_str) {
        None => Ok(CliAction::Run),
        Some("--version") | Some("-V") => Ok(CliAction::Version),
        Some("--help") | Some("-h") => Ok(CliAction::Help),
        Some("--perft") => match args.get(2).and_then(|n| n.parse::<u32>().ok()) {
            Some(depth) => Ok(CliAction::Perft(depth)),
            None => Err(CliError {
                argument: args.get(2).cloned().unwrap_or_else(|| "(missing)".into()),
            }),
        },
        Some(other) => Err(CliError {
            argument: other.to_string(),
        }),
    }
}

pub fn usage() -> &'static str {
    "Usage: pichess [--version] [--perft DEPTH]\n\n  --version      Print version and exit\n  --perft DEPTH  Run start-position perft to DEPTH and print the count"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(rest: &[&str]) -> Vec<String> {
        std::iter::once("pichess".into())
            .chain(rest.iter().map(|s| (*s).to_string()))
            .collect()
    }

    #[test]
    fn default_run() {
        assert_eq!(parse_cli(&args(&[])), Ok(CliAction::Run));
        assert_eq!(parse_cli(&args(&["--version"])), Ok(CliAction::Version));
        assert_eq!(parse_cli(&args(&["--help"])), Ok(CliAction::Help));
        assert_eq!(parse_cli(&args(&["--perft", "4"])), Ok(CliAction::Perft(4)));
        assert_eq!(
            parse_cli(&args(&["--perft"])),
            Err(CliError {
                argument: "(missing)".into()
            })
        );
        assert_eq!(
            parse_cli(&args(&["--perft", "x"])),
            Err(CliError {
                argument: "x".into()
            })
        );
        assert_eq!(
            parse_cli(&args(&["nope"])),
            Err(CliError {
                argument: "nope".into()
            })
        );
    }
}
