//! A collection of [`log4rs`] appenders and configuration for logging in the
//! same style as the Python package [`mlzlog`].
//!
//! [`log4rs`]: https://github.com/sfackler/log4rs
//! [`mlzlog`]: http://pypi.python.org/pypi/mlzlog

pub use log4rs;

use std::env;
use std::fmt;
use std::fs::{DirBuilder, File, OpenOptions, remove_file};
use std::io::{self, Stdout, Write, BufWriter};
use std::path::{Path, PathBuf};
use once_cell::sync::Lazy;
use hashbrown::HashSet;
use parking_lot::Mutex;
#[cfg(target_family = "unix")]
use std::os::unix::fs::symlink;
#[cfg(target_family = "windows")]
use std::os::windows::fs::symlink_file as symlink;

use time::{OffsetDateTime, Time, Duration};
use log::{Level, Record, LevelFilter};
use log4rs::append::Append;
use log4rs::filter::{Filter, Response as FilterResponse};
use log4rs::encode::Encode;
use log4rs::encode::pattern::PatternEncoder;
use log4rs::encode::writer::simple::SimpleWriter;
use log4rs::config::{Config, Root, Appender};
use ansi_term::Colour::{Red, White, Purple};


static HMS: Lazy<Vec<time::format_description::FormatItem>> = Lazy::new(|| {
    time::format_description::parse_borrowed::<2>(r"\[[hour]:[minute]:[second]\]").unwrap()
});

static YMD: Lazy<Vec<time::format_description::FormatItem>> = Lazy::new(|| {
    time::format_description::parse_borrowed::<2>(r"[year]-[month]-[day]").unwrap()
});

fn time_now() -> String {
    OffsetDateTime::now_local().unwrap().format(&HMS).unwrap()
}

fn date_now() -> String {
    OffsetDateTime::now_local().unwrap().format(&YMD).unwrap()
}

fn ensure_dir<P: AsRef<Path>>(path: P) -> io::Result<()> {
    if path.as_ref().is_dir() {
        return Ok(());
    }
    DirBuilder::new().recursive(true).create(path)
}


/// A log4rs appender that writes ANSI colored log messages to stdout.
pub struct ConsoleAppender {
    prefix: String,
    stdout: Stdout,
}

impl fmt::Debug for ConsoleAppender {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.write_str("<console>")
    }
}

impl ConsoleAppender {
    pub fn new(prefix: &str) -> ConsoleAppender {
        ConsoleAppender { prefix: prefix.into(),
                          stdout: io::stdout(), }
    }
}

impl Default for ConsoleAppender {
    fn default() -> Self {
        Self::new("")
    }
}

impl Append for ConsoleAppender {
    fn append(&self, record: &Record) -> anyhow::Result<()> {
        let mut stdout = self.stdout.lock();
        let msg = log_mdc::get("thread", |thread_str| {
            let thread_str = thread_str.unwrap_or("");
            match record.level() {
                Level::Error => Red.bold().paint(
                    format!("{}{}ERROR: {}", self.prefix, thread_str, record.args())),
                Level::Warn  => Purple.paint(
                    format!("{}{}WARNING: {}", self.prefix, thread_str, record.args())),
                Level::Debug => White.paint(
                    format!("{}{}{}", self.prefix, thread_str, record.args())),
                _ => From::from(
                    format!("{}{}{}", self.prefix, thread_str, record.args())),
            }
        });
        writeln!(stdout, "{} {}", White.paint(time_now()), msg)?;
        stdout.flush()?;
        Ok(())
    }

    fn flush(&self) {
        // nothing here, like in upstream appender impl
    }
}


/// A log4rs appender that writes uncolored log messages to stdout.
pub struct PlainConsoleAppender {
    prefix: String,
    stdout: Stdout,
}

impl fmt::Debug for PlainConsoleAppender {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.write_str("<console>")
    }
}

impl PlainConsoleAppender {
    pub fn new(prefix: &str) -> Self {
        Self { prefix: prefix.into(),
               stdout: io::stdout(), }
    }
}

impl Default for PlainConsoleAppender {
    fn default() -> Self {
        Self::new("")
    }
}

impl Append for PlainConsoleAppender {
    fn append(&self, record: &Record) -> anyhow::Result<()> {
        let mut stdout = self.stdout.lock();
        log_mdc::get("thread", |thread_str| {
            let thread_str = thread_str.unwrap_or("");
            match record.level() {
                Level::Error => writeln!(stdout, "{} {}{}ERROR: {}",
                                         time_now(), self.prefix, thread_str, record.args()),
                Level::Warn  => writeln!(stdout, "{} {}{}WARNING: {}",
                                         time_now(), self.prefix, thread_str, record.args()),
                Level::Debug => writeln!(stdout, "{} {}{}DEBUG: {}",
                                         time_now(), self.prefix, thread_str, record.args()),
                _ =>            writeln!(stdout, "{} {}{}{}",
                                         time_now(), self.prefix, thread_str, record.args()),
            }
        })?;
        stdout.flush()?;
        Ok(())
    }

    fn flush(&self) {
        // nothing here, like in upstream appender impl
    }
}


type Writer = SimpleWriter<BufWriter<File>>;

/// A log4rs appender that writes to daily rolling logfiles with the date
/// used as a suffix in the filename.
#[derive(Debug)]
pub struct RollingFileAppender {
    dir:     PathBuf,
    prefix:  String,
    link_fn: PathBuf,
    file:    Mutex<(Option<Writer>, OffsetDateTime)>,
    pattern: PatternEncoder,
}

impl RollingFileAppender {
    pub fn new(dir: &Path, prefix: &str) -> RollingFileAppender {
        let thisday = OffsetDateTime::now_local().unwrap().replace_time(Time::MIDNIGHT);
        let roll_at = thisday + Duration::days(1);
        let pattern = PatternEncoder::new("{d(%H:%M:%S,%f)(local)} : {l:<5} : {X(thread)}{m}{n}");
        let link_fn = dir.join("current");
        let prefix = prefix.replace('/', "-");
        RollingFileAppender { dir: dir.to_path_buf(),
                              prefix,
                              link_fn,
                              file: Mutex::new((None, roll_at)),
                              pattern, }
    }

    fn rollover(&self, file_opt: &mut Option<Writer>, roll_at: &mut OffsetDateTime) -> io::Result<()> {
        file_opt.take(); // will drop the file if open
        let full = format!("{}-{}.log", self.prefix, date_now());
        let new_fn = self.dir.join(full);
        let fp = OpenOptions::new()
            .create(true).write(true).append(true)
            .open(&new_fn)?;
        *file_opt = Some(SimpleWriter(BufWriter::new(fp)));
        let _ = remove_file(&self.link_fn);
        let _ = symlink(new_fn.file_name().unwrap(), &self.link_fn);
        *roll_at += Duration::days(1);
        Ok(())
    }
}

impl Append for RollingFileAppender {
    fn append(&self, record: &Record) -> anyhow::Result<()> {
        let (ref mut file_opt, ref mut roll_at) = *self.file.lock();
        if file_opt.is_none() || OffsetDateTime::now_utc() >= *roll_at {
            self.rollover(file_opt, roll_at)?;
        }
        let fp = file_opt.as_mut().unwrap();
        self.pattern.encode(fp, record)?;
        fp.flush()?;
        Ok(())
    }

    fn flush(&self) {
        // nothing here, like in upstream appender impl
    }
}

#[cfg(feature="systemd")]
use systemd::journal;

/// An appender which logs to the systemd journal.
#[derive(Debug)]
#[cfg(feature="systemd")]
pub struct JournalAppender;

#[cfg(feature="systemd")]
impl Append for JournalAppender {
    fn append(&self, record: &Record) -> anyhow::Result<()> {
        journal::log_record(record);
        Ok(())
    }

    fn flush(&self) { }
}

/// A log4rs filter for filtering by target.
#[derive(Debug, Clone)]
pub struct TargetFilter {
    black: HashSet<String>,
    white: HashSet<String>,
}

impl Filter for TargetFilter {
    fn filter(&self, record: &Record) -> FilterResponse {
        self.filter_inner(record.target())
    }
}

impl TargetFilter {
    fn new(black: HashSet<String>, white: HashSet<String>) -> Self {
        Self { black, white }
    }

    fn filter_inner(&self, target: &str) -> FilterResponse {
        use self::FilterResponse::*;
        if self.black.contains(target) {
            Reject
        } else if self.white.contains(target) {
            Neutral
        } else {
            // no specific entry for this module, try the parent
            target.rsplit_once("::").map_or_else(
                || if self.white.is_empty() { Neutral } else { Reject },
                |(parent, _)| self.filter_inner(parent))
        }
    }
}

fn parse_filter_config(cfg: String) -> TargetFilter {
    let mut black = HashSet::default();
    let mut white = HashSet::default();

    for entry in cfg.split(',') {
        if let Some(rest) = entry.strip_prefix('-') {
            black.insert(rest.into());
        } else if let Some(rest) = entry.strip_prefix('+') {
            white.insert(rest.into());
        } else {
            white.insert(entry.into());
        }
    }

    TargetFilter::new(black, white)
}


/// Settings struct for mlzlog::init().
///
/// It is recommended to use this as
///
/// ```
/// # use mlzlog::Settings;
/// let settings = Settings { show_appname: false, .. Settings::default() };
/// ```
///
/// to avoid code breaking when new options are added.
///
/// If `show_appname` is true, the appname is prepended to console messages.
/// If `debug` is true, debug messages are output.  If `use_stdout` is true, a
/// `ConsoleAppender` is created to log to stdout.  If `use_journal` is true,
/// messages will be written to journald.
#[derive(Debug, Clone)]
pub struct Settings {
    pub show_appname: bool,
    pub debug: bool,
    pub use_stdout: bool,
    pub stdout_color: bool,
    pub use_journal: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            show_appname: true,
            debug: false,
            use_stdout: true,
            stdout_color: true,
            use_journal: false,
        }
    }
}


/// Initialize default mlzlog settings.
///
/// `log_path` is the base path for logfiles.  The `appname` is used as the base
/// name for the logfiles, with the current day appended.  The logfile is rolled
/// over on midnight.  A symbolic link named `current` always links to the
/// latest file.
///
/// If `log_path` is `None`, no logfiles are written to disk.
///
/// The hardcoded setting of `log_path` can be overridden by an environment
/// variabled named `MLZ_LOG_PATH`.  If it is empty, no logfiles are written,
/// else it specifies the new base path.
///
/// Logger target filtering can be configured using the `MLZ_LOG_FILTER`
/// environment variable.  Its syntax is "[+-]?target1,[+-]?target2,..."  where
/// `+` or no prefix enters the target into a whitelist, while `-` enters it
/// into a blacklist.  If there are no whitelist entries, anything not in the
/// blacklist will be let through.
///
/// The black- and whitelists are checked for the record's target and then for
/// its parents (as given by `rust::module::paths`).  The first match wins.
pub fn init<P: AsRef<Path>>(log_path: Option<P>, appname: &str, settings: Settings) -> io::Result<()> {
    let mut config = Config::builder();
    let mut root_cfg = Root::builder();
    let mut log_path = log_path.map(|p| p.as_ref().to_path_buf());

    // override
    if let Some(path) = env::var_os("MLZ_LOG_PATH") {
        if path.is_empty() {
            log_path = None;
        } else {
            log_path = Some(Path::new(&path).to_path_buf());
        }
    }

    let filter = env::var("MLZ_LOG_FILTER").ok().map(parse_filter_config);

    if let Some(p) = log_path {
        ensure_dir(&p)?;
        let file_appender = RollingFileAppender::new(&p, appname);
        root_cfg = root_cfg.appender("file");
        let mut app_builder = Appender::builder();
        if let Some(ref f) = filter {
            app_builder = app_builder.filter(Box::new(f.clone()));
        }
        config = config.appender(app_builder.build("file", Box::new(file_appender)));
    }
    if settings.use_stdout {
        let appname_prefix = format!("[{appname}] ");
        let prefix = if settings.show_appname { &appname_prefix } else { "" };
        root_cfg = root_cfg.appender("con");
        let mut app_builder = Appender::builder();
        if let Some(ref f) = filter {
            app_builder = app_builder.filter(Box::new(f.clone()));
        }
        if settings.stdout_color {
            let con_appender = ConsoleAppender::new(prefix);
            config = config.appender(app_builder.build("con", Box::new(con_appender)));
        } else {
            let con_appender = PlainConsoleAppender::new(prefix);
            config = config.appender(app_builder.build("con", Box::new(con_appender)));
        }
    }
    #[cfg(feature="systemd")]
    {
        if settings.use_journal {
            let mut app_builder = Appender::builder();
            if let Some(ref f) = filter {
                app_builder = app_builder.filter(Box::new(f.clone()));
            }
            config = config.appender(app_builder.build("journal", Box::new(JournalAppender)));
        }
    }
    #[cfg(not(feature="systemd"))]
    {
        if settings.use_journal {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "journal integration requested, but not built into crate".to_string()));
        }
    }
    let config = config.build(root_cfg.build(if settings.debug { LevelFilter::Debug }
                                             else { LevelFilter::Info }))
                       .expect("error building logging config");

    let _ = log4rs::init_config(config);
    Ok(())
}


/// Set logging prefix for the current thread.
///
/// This prefix is prepended to every log message from that thread.
pub fn set_thread_prefix(prefix: impl Into<String>) {
    log_mdc::insert("thread", prefix.into());
}
