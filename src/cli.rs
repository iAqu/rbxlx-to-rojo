use log::info;
use rbxlx_to_rojo::converter::{convert_file, ConvertError};
use std::{
    fmt, fs,
    io::{self, Write},
    path::PathBuf,
    sync::{Arc, RwLock},
};

#[derive(Debug)]
enum Problem {
    Convert(ConvertError),
    IoError(&'static str, io::Error),
    NFDCancel,
    NFDError(String),
}

impl fmt::Display for Problem {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Problem::Convert(error) => write!(formatter, "{}", error),

            Problem::IoError(doing_what, error) => {
                write!(formatter, "While attempting to {}, {}", doing_what, error)
            }

            Problem::NFDCancel => write!(formatter, "Didn't choose a file."),

            Problem::NFDError(error) => write!(
                formatter,
                "Something went wrong when choosing a file: {}",
                error,
            ),
        }
    }
}

impl From<ConvertError> for Problem {
    fn from(error: ConvertError) -> Self {
        Problem::Convert(error)
    }
}

struct WrappedLogger {
    log: env_logger::Logger,
    log_file: Arc<RwLock<Option<fs::File>>>,
}

impl log::Log for WrappedLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        self.log.enabled(metadata)
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            self.log.log(record);

            if let Some(ref mut log_file) = &mut *self.log_file.write().unwrap() {
                log_file
                    .write(format!("{}\r\n", record.args()).as_bytes())
                    .ok();
            }
        }
    }

    fn flush(&self) {}
}

fn routine() -> Result<(), Problem> {
    let env_logger = env_logger::Builder::new()
        .filter_level(log::LevelFilter::Info)
        .build();

    let log_file = Arc::new(RwLock::new(None));
    let logger = WrappedLogger {
        log: env_logger,
        log_file: Arc::clone(&log_file),
    };

    log::set_boxed_logger(Box::new(logger)).unwrap();
    log::set_max_level(log::LevelFilter::Info);

    info!("rbxlx-to-rojo {}", env!("CARGO_PKG_VERSION"));

    info!("Select a place file.");
    let file_path = PathBuf::from(match std::env::args().nth(1) {
        Some(text) => text,
        None => match nfd::open_file_dialog(Some("rbxl,rbxm,rbxlx,rbxmx"), None)
            .map_err(|error| Problem::NFDError(error.to_string()))?
        {
            nfd::Response::Okay(path) => path,
            nfd::Response::Cancel => Err(Problem::NFDCancel)?,
            _ => unreachable!(),
        },
    });

    info!("Select the path to put your Rojo project in.");
    let root = PathBuf::from(match std::env::args().nth(2) {
        Some(text) => text,
        None => match nfd::open_pick_folder(Some(&file_path.parent().unwrap().to_string_lossy()))
            .map_err(|error| Problem::NFDError(error.to_string()))?
        {
            nfd::Response::Okay(path) => path,
            nfd::Response::Cancel => Err(Problem::NFDCancel)?,
            _ => unreachable!(),
        },
    });

    log_file.write().unwrap().replace(
        fs::File::create(root.join("rbxlx-to-rojo.log"))
            .map_err(|error| Problem::IoError("couldn't create log file", error))?,
    );

    convert_file(&file_path, &root, |message| info!("{}", message))?;
    Ok(())
}

fn main() {
    if let Err(error) = routine() {
        eprintln!("An error occurred while using rbxlx-to-rojo.");
        eprintln!("{}", error);
    }
}
