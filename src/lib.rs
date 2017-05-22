//! A collection of [`log4rs`] appenders and configuration for logging in the
//! same style as the Python package [`mlzlog`].
//!
//! [`log4rs`]: https://github.com/sfackler/log4rs
//! [`mlzlog`]: http://pypi.python.org/pypi/mlzlog

pub extern crate log4rs;

extern crate log;
extern crate time;
extern crate ansi_term;
extern crate parking_lot;

use std::fmt;
use std::error::Error;
use std::fs::{DirBuilder, File, OpenOptions, remove_file};
use std::io::{self, Stdout, Write, BufWriter};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use parking_lot::Mutex;

use time::{Timespec, Tm, Duration, get_time, now, strftime};
use log::{LogLevel, LogRecord, LogLevelFilter};
use log4rs::append::Append;
use log4rs::encode::Encode;
use log4rs::encode::pattern::PatternEncoder;
use log4rs::encode::writer::simple::SimpleWriter;
use log4rs::config::{Config, Root, Appender};
use ansi_term::Colour::{Red, White, Purple};


fn ensure_dir<P: AsRef<Path>>(path: P) -> io::Result<()> {
    if path.as_ref().is_dir() {
        return Ok(());
    }
    DirBuilder::new().recursive(true).create(path)
}

fn open_file<P: AsRef<Path>>(path: P, mode: &str) -> io::Result<File> {
    let mut opt = OpenOptions::new();
    for ch in mode.chars() {
        match ch {
            'r' => { opt.read(true); },
            'w' => { opt.write(true).create(true); },
            'a' => { opt.write(true).append(true); },
            _   => { },  // ignore unsupported chars
        }
    }
    opt.open(path)
}

/// A log4rs appender that writes ANSI colored log messages to stdout.
pub struct ConsoleAppender {
    stdout: Stdout,
}

impl fmt::Debug for ConsoleAppender {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.write_str("<console>")
    }
}

impl ConsoleAppender {
    pub fn new() -> ConsoleAppender {
        ConsoleAppender { stdout: io::stdout() }
    }
}

impl Default for ConsoleAppender {
    fn default() -> Self {
        Self::new()
    }
}

impl Append for ConsoleAppender {
    fn append(&self, record: &LogRecord) -> Result<(), Box<Error + Send + Sync>> {
        let mut stdout = self.stdout.lock();
        let time_str = strftime("[%H:%M:%S]", &now()).unwrap();
        let msg = match record.level() {
            LogLevel::Error => Red.bold().paint(
                format!("ERROR: {}", record.args())),
            LogLevel::Warn  => Purple.paint(
                format!("WARNING: {}", record.args())),
            LogLevel::Debug => White.paint(format!("{}", record.args())),
            _ => From::from(format!("{}", record.args())),
        };
        try!(writeln!(stdout, "{} {}", White.paint(time_str), msg));
        try!(stdout.flush());
        Ok(())
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
    file:    Mutex<(Option<Writer>, Timespec)>,
    pattern: PatternEncoder,
}

impl RollingFileAppender {
    pub fn new(dir: &Path, prefix: &str) -> RollingFileAppender {
        let thisday = Tm { tm_hour: 0, tm_min: 0, tm_sec: 0, tm_nsec: 0, ..now() };
        let roll_at = (thisday + Duration::days(1)).to_timespec();
        let pattern = PatternEncoder::new("{d(%H:%M:%S,%f)(local)} : {l:<5} : {m}{n}");
        let link_fn = dir.join("current");
        let prefix = prefix.replace("/", "-");
        RollingFileAppender { dir: dir.to_path_buf(), prefix, link_fn,
                              file: Mutex::new((None, roll_at)), pattern }
    }

    fn rollover(&self, file_opt: &mut Option<Writer>, roll_at: &mut Timespec) -> io::Result<()> {
        file_opt.take();  // will drop the file if open
        let time = strftime("%Y-%m-%d", &now()).unwrap();
        let full = format!("{}-{}.log", self.prefix, time);
        let new_fn = self.dir.join(full);
        let fp = try!(open_file(&new_fn, "wa"));
        *file_opt = Some(SimpleWriter(BufWriter::new(fp)));
        let _ = remove_file(&self.link_fn);
        let _ = symlink(&new_fn.file_name().unwrap(), &self.link_fn);
        *roll_at = *roll_at + Duration::days(1);
        Ok(())
    }
}

impl Append for RollingFileAppender {
    fn append(&self, record: &LogRecord) -> Result<(), Box<Error + Send + Sync>> {
        let (ref mut file_opt, ref mut roll_at) = *self.file.lock();
        if file_opt.is_none() || get_time() >= *roll_at {
            try!(self.rollover(file_opt, roll_at));
        }
        let fp = file_opt.as_mut().unwrap();
        try!(self.pattern.encode(fp, record));
        try!(fp.flush());
        Ok(())
    }
}


/// Initialize default mlzlog settings.
///
/// `log_path` is the base path for logfiles, in which a subdirectory `appname`
/// is created for this application.  The application name is at the same time
/// used as the base name for the logfiles.
///
/// If `debug` is true, debug messages are logged.
/// If `use_stdout` is true, a `ConsoleAppender` is created to log to stdout.
pub fn init<P: AsRef<Path>>(log_path: P, appname: &str, debug: bool,
                            use_stdout: bool) -> io::Result<()> {
    try!(ensure_dir(log_path.as_ref()));

    let file_appender = RollingFileAppender::new(log_path.as_ref(), appname);
    let mut root_cfg = Root::builder().appender("file");
    if use_stdout {
        root_cfg = root_cfg.appender("con");
    }
    let mut config = Config::builder()
        .appender(Appender::builder().build("file", Box::new(file_appender)));
    if use_stdout {
        let con_appender = ConsoleAppender::new();
        config = config.appender(Appender::builder().build("con", Box::new(con_appender)));
    }
    let config = config.build(root_cfg.build(if debug { LogLevelFilter::Debug }
                                             else { LogLevelFilter::Info }))
                       .expect("error building logging config");

    let _ = log4rs::init_config(config);
    Ok(())
}
