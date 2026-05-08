use crate::{filesystem::FileSystem, process_instructions, structures::*};
use log::info;
use pretty_assertions::assert_eq;
use rbx_dom_weak::types::Variant;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::ErrorKind,
    time::Instant,
};

#[derive(Deserialize, Serialize, Debug, PartialEq)]
enum VirtualFileContents {
    Bytes(String),
    Instance(HashMap<String, Variant>),
    Vfs(VirtualFileSystem),
}

#[derive(Deserialize, Serialize, Debug, PartialEq)]
struct VirtualFile {
    contents: VirtualFileContents,
}

#[derive(Deserialize, Serialize, Debug, Default)]
struct VirtualFileSystem {
    files: BTreeMap<String, VirtualFile>,
    tree: BTreeMap<String, TreePartition>,
    #[serde(skip)]
    finished: bool,
}

impl PartialEq<VirtualFileSystem> for VirtualFileSystem {
    fn eq(&self, rhs: &VirtualFileSystem) -> bool {
        self.files == rhs.files && self.tree == rhs.tree
    }
}

impl InstructionReader for VirtualFileSystem {
    fn finish_instructions(&mut self) {
        self.finished = true;
    }

    fn read_instruction<'a>(&mut self, instruction: Instruction<'a>) {
        match instruction {
            Instruction::AddToTree { name, partition } => {
                self.tree.insert(name, partition);
            }

            Instruction::CreateFile { filename, contents } => {
                let parent = filename
                    .parent()
                    .expect("no parent?")
                    .to_string_lossy()
                    .replace("\\", "/");
                let filename = filename
                    .file_name()
                    .expect("no filename?")
                    .to_string_lossy()
                    .replace("\\", "/");

                let system = if parent == "" {
                    self
                } else {
                    match self
                        .files
                        .get_mut(&parent)
                        .unwrap_or_else(|| panic!("no folder for {:?}", parent))
                        .contents
                    {
                        VirtualFileContents::Vfs(ref mut system) => system,
                        _ => unreachable!("attempt to parent to a file"),
                    }
                };

                let contents_string = String::from_utf8_lossy(&contents).into_owned();
                let rbxmx = filename.ends_with(".rbxmx");
                system.files.insert(
                    filename,
                    VirtualFile {
                        contents: if rbxmx {
                            let tree = rbx_xml::from_str_default(&contents_string)
                                .expect("couldn't decode encoded xml");
                            let child_id = tree.root().children()[0];
                            let child_instance = tree.get_by_ref(child_id).unwrap();
                            VirtualFileContents::Instance(
                                child_instance
                                    .properties
                                    .iter()
                                    .map(|(key, value)| (key.to_string(), value.clone()))
                                    .collect(),
                            )
                        } else {
                            VirtualFileContents::Bytes(contents_string)
                        },
                    },
                );
            }

            Instruction::CreateFolder { folder } => {
                let name = folder.to_string_lossy().replace("\\", "/");
                self.files.insert(
                    name,
                    VirtualFile {
                        contents: VirtualFileContents::Vfs(VirtualFileSystem::default()),
                    },
                );
            }
        }
    }
}

#[test]
fn run_tests() {
    let _ = env_logger::builder().is_test(true).try_init();
    for entry in fs::read_dir("./test-files").expect("couldn't read test-files") {
        let entry = entry.unwrap();
        let path = entry.path();
        info!("testing {:?}", path);

        let mut source_path = path.clone();
        source_path.push("source.rbxmx");
        let source = fs::read_to_string(&source_path).expect("couldn't read source.rbxmx");

        let time = Instant::now();
        let tree = rbx_xml::from_str_default(&source).expect("couldn't deserialize source.rbxmx");
        info!(
            "decoding for {:?} took {}ms",
            path,
            Instant::now().duration_since(time).as_millis()
        );

        let mut vfs = VirtualFileSystem::default();
        let time = Instant::now();
        process_instructions(&tree, &mut vfs);
        info!(
            "processing instructions for {:?} took {}ms",
            path,
            Instant::now().duration_since(time).as_millis()
        );

        let mut expected_path = path.clone();
        expected_path.push("output.json");
        assert!(vfs.finished, "finish_instructions was not called");

        if let Ok(expected) = fs::read_to_string(&expected_path) {
            assert_eq!(
                serde_json::from_str::<VirtualFileSystem>(&expected).unwrap(),
                vfs,
            );
        } else {
            let output = serde_json::to_string_pretty(&vfs).unwrap();
            fs::write(&expected_path, output).expect("couldn't write to output.json");
        }

        let filesystem_path = path.join("filesystem");
        if let Err(error) = fs::remove_dir_all(&filesystem_path) {
            match error.kind() {
                ErrorKind::NotFound => {}
                other => panic!("couldn't remove filesystem dir: {:?}", other),
            }
        }

        fs::create_dir(&filesystem_path).unwrap();

        let mut filesystem = FileSystem::from_root(filesystem_path);
        process_instructions(&tree, &mut filesystem);
    }
}

#[test]
fn convert_file_rejects_unknown_extension() {
    let _ = env_logger::builder().is_test(true).try_init();
    let input = std::path::Path::new("test-files/baseplate/source.txt");
    let output = std::path::Path::new("/private/tmp/rbxlx-to-rojo-test-output");

    let error = crate::converter::convert_file(input, output, |_| {}).unwrap_err();

    assert_eq!(
        error.to_string(),
        "The file provided does not have a recognized file extension"
    );
}

#[test]
fn conversion_output_folder_uses_input_file_stem() {
    let source = std::path::Path::new("test-files/folder-with-value/source.rbxmx");
    let output_root = std::env::temp_dir().join(format!(
        "rbxlx-to-rojo-converter-test-{}",
        std::process::id()
    ));

    if output_root.exists() {
        std::fs::remove_dir_all(&output_root).unwrap();
    }
    std::fs::create_dir_all(&output_root).unwrap();

    let result = crate::converter::convert_file(source, &output_root, |_| {}).unwrap();

    assert_eq!(result, output_root.join("source"));
    assert!(result.join("default.project.json").exists());

    std::fs::remove_dir_all(&output_root).unwrap();
}

#[test]
fn convert_file_accepts_uppercase_extension() {
    let source = std::path::Path::new("test-files/folder-with-value/source.rbxmx");
    let temp_root = std::env::temp_dir().join(format!(
        "rbxlx-to-rojo-uppercase-test-{}",
        std::process::id()
    ));
    let input_path = temp_root.join("SOURCE.RBXMX");
    let output_root = temp_root.join("output");

    if temp_root.exists() {
        std::fs::remove_dir_all(&temp_root).unwrap();
    }
    std::fs::create_dir_all(&output_root).unwrap();
    std::fs::copy(source, &input_path).unwrap();

    let result = crate::converter::convert_file(&input_path, &output_root, |_| {}).unwrap();

    assert_eq!(result, output_root.join("SOURCE"));
    assert!(result.join("default.project.json").exists());

    std::fs::remove_dir_all(&temp_root).unwrap();
}

#[test]
fn convert_file_returns_error_when_output_src_is_a_file() {
    let source = std::path::Path::new("test-files/folder-with-value/source.rbxmx");
    let output_root = std::env::temp_dir().join(format!(
        "rbxlx-to-rojo-converter-output-error-test-{}",
        std::process::id()
    ));
    let project_path = output_root.join("source");

    if output_root.exists() {
        std::fs::remove_dir_all(&output_root).unwrap();
    }
    std::fs::create_dir_all(&project_path).unwrap();
    std::fs::write(project_path.join("src"), "not a directory").unwrap();

    let error = crate::converter::convert_file(source, &output_root, |_| {}).unwrap_err();

    assert!(matches!(error, crate::converter::ConvertError::Io(_, _)));
    assert!(
        error.to_string().contains("create the source folder"),
        "{}",
        error
    );

    std::fs::remove_dir_all(&output_root).unwrap();
}
