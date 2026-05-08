use crate::structures::*;
use serde::{ser::SerializeMap, Serialize, Serializer};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Write},
    path::PathBuf,
};

const SRC: &str = "src";

fn serialize_project_tree<S: Serializer>(
    tree: &BTreeMap<String, TreePartition>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(Some(tree.len() + 1))?;
    map.serialize_entry("$className", "DataModel")?;
    for (k, v) in tree {
        map.serialize_entry(k, v)?;
    }
    map.end()
}

#[derive(Clone, Debug, Serialize)]
struct Project {
    name: String,
    #[serde(serialize_with = "serialize_project_tree")]
    tree: BTreeMap<String, TreePartition>,
}

impl Project {
    fn new() -> Self {
        Self {
            name: "project".to_string(),
            tree: BTreeMap::new(),
        }
    }
}

#[derive(Debug)]
pub struct FileSystem {
    project: Project,
    root: PathBuf,
    source: PathBuf,
    error: Option<(&'static str, io::Error)>,
}

impl FileSystem {
    pub fn from_root(root: PathBuf) -> Self {
        let source = root.join(SRC);
        let project = Project::new();

        let error = match fs::create_dir(&source) {
            Ok(()) => None,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists && source.is_dir() => None,
            Err(error) => Some(("create the source folder", error)),
        };

        Self {
            project,
            root,
            source,
            error,
        }
    }

    pub fn into_error(self) -> Option<(&'static str, io::Error)> {
        self.error
    }

    fn record_error(&mut self, doing_what: &'static str, error: io::Error) {
        if self.error.is_none() {
            self.error = Some((doing_what, error));
        }
    }
}

impl InstructionReader for FileSystem {
    fn read_instruction<'a>(&mut self, instruction: Instruction<'a>) {
        if self.error.is_some() {
            return;
        }

        match instruction {
            Instruction::AddToTree {
                name,
                mut partition,
            } => {
                assert!(
                    self.project.tree.get(&name).is_none(),
                    "Duplicate item added to tree! Instances can't have the same name: {}",
                    name
                );

                if let Some(path) = partition.path {
                    partition.path = Some(PathBuf::from(SRC).join(path));
                }

                for child in partition.children.values_mut() {
                    if let Some(path) = &child.path {
                        child.path = Some(PathBuf::from(SRC).join(path));
                    }
                }

                self.project.tree.insert(name, partition);
            }

            Instruction::CreateFile { filename, contents } => {
                match File::create(self.source.join(&filename)) {
                    Ok(mut file) => {
                        if let Err(error) = file.write_all(&contents) {
                            self.record_error("write an output file", error);
                        }
                    }
                    Err(error) => self.record_error("create an output file", error),
                }
            }

            Instruction::CreateFolder { folder } => {
                if let Err(error) = fs::create_dir_all(self.source.join(&folder)) {
                    self.record_error("create an output folder", error);
                }
            }
        }
    }

    fn finish_instructions(&mut self) {
        if self.error.is_some() {
            return;
        }

        let project = match serde_json::to_string_pretty(&self.project) {
            Ok(project) => project,
            Err(error) => {
                self.record_error(
                    "serialize the project file",
                    io::Error::new(io::ErrorKind::Other, error),
                );
                return;
            }
        };

        match File::create(self.root.join("default.project.json")) {
            Ok(mut file) => {
                if let Err(error) = file.write_all(project.as_bytes()) {
                    self.record_error("write the project file", error);
                }
            }
            Err(error) => self.record_error("create the project file", error),
        }
    }
}
