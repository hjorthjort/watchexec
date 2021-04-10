//! CLI arguments and library Config struct
//!
//! The [`Config`] struct is not constructable, use [`ConfigBuilder`].
//!
//! # Examples
//!
//! ```
//! # use watchexec::cli::ConfigBuilder;
//! ConfigBuilder::default()
//!     .cmd(vec!["echo hello world".into()])
//!     .paths(vec![".".into()])
//!     .build()
//!     .expect("mission failed");
//! ```

use crate::error;
use clap::{App, Arg};
use log::LevelFilter;
use std::{
    ffi::OsString,
    path::{PathBuf, MAIN_SEPARATOR},
    process::Command,
};

use crate::config::{Config, ConfigBuilder};

#[deprecated(since = "1.15.0", note = "Config has moved to config::Config")]
pub type Args = Config;

#[deprecated(since = "1.15.0", note = "ConfigBuilder has moved to config::ConfigBuilder")]
pub type ArgsBuilder = ConfigBuilder;

/// Clear the screen.
#[cfg(target_family = "windows")]
pub fn clear_screen() {
// TODO: clearscreen with powershell?
    let _ = Command::new("cmd")
        .arg("/c")
        .arg("tput reset || cls")
        .status();
}

/// Clear the screen.
#[cfg(target_family = "unix")]
pub fn clear_screen() {
// TODO: clear screen via control codes instead
    let _ = Command::new("tput").arg("reset").status();
}

#[deprecated(since = "1.15.0", note = "this will be removed from the library API. use the builder")]
pub fn get_args() -> error::Result<(Config, LevelFilter)> {
    get_args_impl(None::<&[&str]>)
}

#[deprecated(since = "1.15.0", note = "this will be removed from the library API. use the builder")]
pub fn get_args_from<I, T>(from: I) -> error::Result<(Config, LevelFilter)>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    get_args_impl(Some(from))
}

fn get_args_impl<I, T>(from: Option<I>) -> error::Result<(Config, LevelFilter)>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let app = App::new("watchexec")
        .version(crate_version!())
        .about("Execute commands when watched files change")
        .arg(Arg::with_name("command")
                 .help("Command to execute")
                 .multiple(true)
                 .required(true))
        .arg(Arg::with_name("extensions")
                 .help("Comma-separated list of file extensions to watch (js,css,html)")
                 .short("e")
                 .long("exts")
                 .takes_value(true))
        .arg(Arg::with_name("path")
                 .help("Watch a specific file or directory")
                 .short("w")
                 .long("watch")
                 .number_of_values(1)
                 .multiple(true)
                 .takes_value(true))
        .arg(Arg::with_name("clear")
                 .help("Clear screen before executing command")
                 .short("c")
                 .long("clear"))
        .arg(Arg::with_name("restart")
                 .help("Restart the process if it's still running")
                 .short("r")
                 .long("restart"))
        .arg(Arg::with_name("signal")
                 .help("Send signal to process upon changes, e.g. SIGHUP")
                 .short("s")
                 .long("signal")
                 .takes_value(true)
                 .number_of_values(1)
                 .value_name("signal"))
        .arg(Arg::with_name("kill")
                 .hidden(true)
                 .short("k")
                 .long("kill"))
        .arg(Arg::with_name("debounce")
                 .help("Set the timeout between detected change and command execution, defaults to 500ms")
                 .takes_value(true)
                 .value_name("milliseconds")
                 .short("d")
                 .long("debounce"))
        .arg(Arg::with_name("verbose")
                 .help("Print debugging messages to stderr")
                 .short("v")
                 .long("verbose"))
        .arg(Arg::with_name("changes")
                 .help("Only print path change information. Overridden by --verbose")
                 .long("changes-only"))
        .arg(Arg::with_name("filter")
                 .help("Ignore all modifications except those matching the pattern")
                 .short("f")
                 .long("filter")
                 .number_of_values(1)
                 .multiple(true)
                 .takes_value(true)
                 .value_name("pattern"))
        .arg(Arg::with_name("ignore")
                 .help("Ignore modifications to paths matching the pattern")
                 .short("i")
                 .long("ignore")
                 .number_of_values(1)
                 .multiple(true)
                 .takes_value(true)
                 .value_name("pattern"))
        .arg(Arg::with_name("no-vcs-ignore")
                 .help("Skip auto-loading of .gitignore files for filtering")
                 .long("no-vcs-ignore"))
        .arg(Arg::with_name("no-ignore")
                 .help("Skip auto-loading of ignore files (.gitignore, .ignore, etc.) for filtering")
                 .long("no-ignore"))
        .arg(Arg::with_name("no-default-ignore")
                 .help("Skip auto-ignoring of commonly ignored globs")
                 .long("no-default-ignore"))
        .arg(Arg::with_name("postpone")
                 .help("Wait until first change to execute command")
                 .short("p")
                 .long("postpone"))
        .arg(Arg::with_name("poll")
                 .help("Force polling mode (interval in milliseconds)")
                 .long("force-poll")
                 .value_name("interval"))
        .arg(Arg::with_name("no-shell")
                 .help("Do not wrap command in 'sh -c' resp. 'cmd.exe /C'")
                 .short("n")
                 .long("no-shell"))
        .arg(Arg::with_name("no-meta")
                 .help("Ignore metadata changes")
                 .long("no-meta"))
        .arg(Arg::with_name("no-environment")
                 .help("Do not set WATCHEXEC_*_PATH environment variables for child process")
                 .long("no-environment"))
        .arg(Arg::with_name("once").short("1").hidden(true))
        .arg(Arg::with_name("watch-when-idle")
                 .help("Ignore events while the process is still running")
                 .short("W")
                 .long("watch-when-idle"));

    let args = match from {
        None => app.get_matches(),
        Some(i) => app.get_matches_from(i),
    };

    let mut builder = ConfigBuilder::default();

    let cmd: Vec<String> = values_t!(args.values_of("command"), String).map_err(|err| err.to_string())?;
    builder.cmd(cmd);

    let paths: Vec<PathBuf> = values_t!(args.values_of("path"), String)
        .unwrap_or_else(|_| vec![".".into()])
        .iter()
        .map(|string_path| string_path.into())
        .collect();
    builder.paths(paths);

    // Treat --kill as --signal SIGKILL (for compatibility with deprecated syntax)
    if args.is_present("kill") {
        builder.signal("SIGKILL");
    }

    if let Some(signal) = args.value_of("signal") {
        builder.signal(signal);
    }

    let mut filters = values_t!(args.values_of("filter"), String).unwrap_or_else(|_| Vec::new());
    if let Some(extensions) = args.values_of("extensions") {
        for exts in extensions { // TODO: refactor with flatten()
            filters.extend(exts.split(',').filter_map(|ext| {
                if ext.is_empty() {
                    None
                } else {
                    Some(format!("*.{}", ext.replace(".", "")))
                }
            }));
        }
    }

    builder.filters(filters);

    let mut ignores = vec![];
    let default_ignores = vec![
        format!("**{}.DS_Store", MAIN_SEPARATOR),
        String::from("*.py[co]"),
        String::from("#*#"),
        String::from(".#*"),
        String::from(".*.kate-swp"),
        String::from(".*.sw?"),
        String::from(".*.sw?x"),
        format!("**{}.git{}**", MAIN_SEPARATOR, MAIN_SEPARATOR),
        format!("**{}.hg{}**", MAIN_SEPARATOR, MAIN_SEPARATOR),
        format!("**{}.svn{}**", MAIN_SEPARATOR, MAIN_SEPARATOR),
    ];

    if args.occurrences_of("no-default-ignore") == 0 {
        ignores.extend(default_ignores)
    };
    ignores.extend(values_t!(args.values_of("ignore"), String).unwrap_or_else(|_| Vec::new()));

    builder.ignores(ignores);

    if args.occurrences_of("poll") > 0 {
        builder.poll_interval(value_t!(args.value_of("poll"), u32).unwrap_or_else(|e| e.exit()));
    }

    if args.occurrences_of("debounce") > 0 {
        builder.debounce(value_t!(args.value_of("debounce"), u64).unwrap_or_else(|e| e.exit()));
    }

    // TODO: check how postpone + signal behaves

    builder.clear_screen(args.is_present("clear"));
    builder.restart(args.is_present("restart"));
    builder.run_initially(!args.is_present("postpone"));
    builder.no_shell(args.is_present("no-shell"));
    builder.no_meta(args.is_present("no-meta"));
    builder.no_environment(args.is_present("no-environment"));
    builder.no_vcs_ignore(args.is_present("no-vcs-ignore"));
    builder.no_ignore(args.is_present("no-ignore"));
    builder.poll(args.occurrences_of("poll") > 0);
    builder.watch_when_idle(args.is_present("watch-when-idle"));

    let mut config = builder.build()?;
    if args.is_present("once") {
        config.once = true;
    }

    let loglevel = if args.is_present("verbose") {
        LevelFilter::Debug
    } else if args.is_present("changes") {
        LevelFilter::Info
    } else {
        LevelFilter::Warn
    };

    Ok((config, loglevel))
}
