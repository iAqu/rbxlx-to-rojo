use crate::{filesystem::FileSystem, process_instructions};
use rbx_dom_weak::WeakDom;
use std::{
    fmt, fs,
    io::{self, BufReader},
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub enum ConvertError {
    BinaryDecode(rbx_binary::DecodeError),
    InvalidFile,
    Io(&'static str, io::Error),
    XmlDecode(rbx_xml::DecodeError),
}

impl fmt::Display for ConvertError {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ConvertError::BinaryDecode(error) => write!(
                formatter,
                "While attempting to decode the place file, at {} rbx_binary didn't know what to do",
                error,
            ),

            ConvertError::InvalidFile => write!(
                formatter,
                "The file provided does not have a recognized file extension"
            ),

            ConvertError::Io(doing_what, error) => {
                write!(formatter, "While attempting to {}, {}", doing_what, error)
            }

            ConvertError::XmlDecode(error) => write!(
                formatter,
                "While attempting to decode the place file, at {} rbx_xml didn't know what to do",
                error,
            ),
        }
    }
}

impl std::error::Error for ConvertError {}

pub fn decode_file(path: &Path) -> Result<WeakDom, ConvertError> {
    let is_xml = match path.extension().and_then(|extension| extension.to_str()) {
        Some("rbxmx") | Some("rbxlx") => true,
        Some("rbxm") | Some("rbxl") => false,
        _ => return Err(ConvertError::InvalidFile),
    };

    let file_source = BufReader::new(
        fs::File::open(path).map_err(|error| ConvertError::Io("read the place file", error))?,
    );

    if is_xml {
        rbx_xml::from_reader_default(file_source).map_err(ConvertError::XmlDecode)
    } else {
        rbx_binary::from_reader(file_source).map_err(ConvertError::BinaryDecode)
    }
}

pub fn output_project_path(input_path: &Path, output_root: &Path) -> PathBuf {
    output_root.join(
        input_path
            .file_stem()
            .expect("input path does not have a file stem"),
    )
}

pub fn convert_file(
    input_path: &Path,
    output_root: &Path,
    mut report: impl FnMut(&'static str),
) -> Result<PathBuf, ConvertError> {
    report("Opening place file");
    report("Decoding place file, this is the longest part...");
    let tree = decode_file(input_path)?;

    let project_path = output_project_path(input_path, output_root);
    fs::create_dir_all(&project_path)
        .map_err(|error| ConvertError::Io("create the project folder", error))?;
    let mut filesystem = FileSystem::from_root(project_path.clone());

    report("Starting processing, please wait a bit...");
    process_instructions(&tree, &mut filesystem);
    report("Done");

    Ok(project_path)
}
