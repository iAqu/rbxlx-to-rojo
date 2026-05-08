# macOS App Wrapper Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a small macOS single-window app that wraps the existing `rbxlx-to-rojo` converter with file drag-and-drop, output selection, logs, and Finder reveal.

**Architecture:** Extract CLI conversion into a shared library API, then add a feature-gated `egui/eframe` GUI binary that calls that API on a background thread. Keep CLI behavior intact and keep GUI dependencies optional so normal tests stay lightweight.

**Tech Stack:** Rust 2018, `rbx_xml/rbx_binary`, `eframe/egui`, `rfd`, shell `.app` bundle script.

---

## File Structure

- `src/converter.rs`: shared conversion API, file extension detection, decoding, progress reporting, and output path creation.
- `src/lib.rs`: expose `converter` plus existing conversion internals.
- `src/cli.rs`: replace duplicated decode/output flow with `converter::convert_file`.
- `src/gui.rs`: `eframe` app state, drag-and-drop handling, dialogs, background conversion thread, log/status rendering.
- `src/mac_app.rs`: GUI binary entrypoint.
- `Cargo.toml`: add optional GUI dependencies, `gui` feature, and `rbxlx-to-rojo-gui` binary.
- `scripts/build-mac-app.sh`: build release GUI binary and assemble `dist/rbxlx-to-rojo.app`.
- `docs/superpowers/specs/2026-05-08-mac-app-design.md`: already written design reference.

---

### Task 1: Shared Conversion API

**Files:**
- Create: `src/converter.rs`
- Modify: `src/lib.rs`
- Modify: `src/cli.rs`
- Test: `src/tests.rs`

- [ ] **Step 1: Write failing tests for extension validation and output path**

Add this to `src/tests.rs`:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
CARGO_NET_GIT_FETCH_WITH_CLI=true cargo test convert_file_rejects_unknown_extension
CARGO_NET_GIT_FETCH_WITH_CLI=true cargo test conversion_output_folder_uses_input_file_stem
```

Expected: compile failure because `crate::converter` does not exist.

- [ ] **Step 3: Create shared converter module**

Create `src/converter.rs`:

```rust
use crate::{filesystem::FileSystem, process_instructions};
use rbx_dom_weak::WeakDom;
use std::{
    borrow::Cow,
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
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConvertError::BinaryDecode(error) => write!(
                formatter,
                "While attempting to decode the place file, at {} rbx_binary didn't know what to do",
                error
            ),
            ConvertError::InvalidFile => {
                write!(formatter, "The file provided does not have a recognized file extension")
            }
            ConvertError::Io(doing_what, error) => {
                write!(formatter, "While attempting to {}, {}", doing_what, error)
            }
            ConvertError::XmlDecode(error) => write!(
                formatter,
                "While attempting to decode the place file, at {} rbx_xml didn't know what to do",
                error
            ),
        }
    }
}

impl std::error::Error for ConvertError {}

pub type ProgressReporter<'a> = dyn FnMut(&str) + Send + 'a;

pub fn decode_file(path: &Path) -> Result<WeakDom, ConvertError> {
    let file_source = BufReader::new(
        fs::File::open(path).map_err(|error| ConvertError::Io("read the place file", error))?,
    );

    match path.extension().map(|extension| extension.to_string_lossy()) {
        Some(Cow::Borrowed("rbxmx")) | Some(Cow::Borrowed("rbxlx")) => {
            rbx_xml::from_reader_default(file_source).map_err(ConvertError::XmlDecode)
        }
        Some(Cow::Borrowed("rbxm")) | Some(Cow::Borrowed("rbxl")) => {
            rbx_binary::from_reader(file_source).map_err(ConvertError::BinaryDecode)
        }
        _ => Err(ConvertError::InvalidFile),
    }
}

pub fn output_project_path(input_path: &Path, output_root: &Path) -> PathBuf {
    output_root.join(input_path.file_stem().expect("input file should have a stem"))
}

pub fn convert_file(
    input_path: &Path,
    output_root: &Path,
    mut report: impl FnMut(&str) + Send,
) -> Result<PathBuf, ConvertError> {
    report("Opening place file");
    report("Decoding place file, this is the longest part...");
    let tree = decode_file(input_path)?;

    report("Starting processing, please wait a bit...");
    let project_path = output_project_path(input_path, output_root);
    let mut filesystem = FileSystem::from_root(project_path.clone());
    process_instructions(&tree, &mut filesystem);

    report("Done");
    Ok(project_path)
}
```

Modify `src/lib.rs` near the existing modules:

```rust
pub mod converter;
pub mod filesystem;
pub mod structures;
```

- [ ] **Step 4: Update CLI to use shared API**

In `src/cli.rs`, remove `BinaryDecodeError`, `InvalidFile`, and `XMLDecodeError` from `Problem`, and add:

```rust
ConvertError(rbxlx_to_rojo::converter::ConvertError),
```

Update the display match with:

```rust
Problem::ConvertError(error) => write!(formatter, "{}", error),
```

Replace the manual file open/decode/process block in `routine` with:

```rust
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

rbxlx_to_rojo::converter::convert_file(&file_path, &root, |message| info!("{}", message))
    .map_err(Problem::ConvertError)?;
info!("Done! Check rbxlx-to-rojo.log for a full log.");
```

Remove now-unused imports `BufReader` and `Cow` from `src/cli.rs`.

- [ ] **Step 5: Run tests**

Run:

```bash
CARGO_NET_GIT_FETCH_WITH_CLI=true cargo test
```

Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/converter.rs src/lib.rs src/cli.rs src/tests.rs Cargo.toml Cargo.lock test-files/physical-properties-acoustic-absorption
git commit -m "Extract shared conversion API"
```

---

### Task 2: GUI Feature and Single-Window App

**Files:**
- Modify: `Cargo.toml`
- Create: `src/gui.rs`
- Create: `src/mac_app.rs`

- [ ] **Step 1: Add optional GUI dependencies**

Modify `Cargo.toml`:

```toml
[[bin]]
name = "rbxlx-to-rojo-gui"
path = "src/mac_app.rs"
required-features = ["gui"]

[dependencies]
eframe = { version = "0.31", optional = true }
rfd = { version = "0.15", optional = true }

[features]
cli = ["nfd"]
gui = ["eframe", "rfd"]
```

Keep the existing dependencies and existing CLI binary.

- [ ] **Step 2: Create GUI binary entrypoint**

Create `src/mac_app.rs`:

```rust
fn main() -> eframe::Result<()> {
    rbxlx_to_rojo::gui::run()
}
```

Modify `src/lib.rs`:

```rust
#[cfg(feature = "gui")]
pub mod gui;
```

- [ ] **Step 3: Implement app state and background conversion**

Create `src/gui.rs`:

```rust
use crate::converter::convert_file;
use eframe::egui;
use std::{
    path::PathBuf,
    process::Command,
    sync::mpsc::{self, Receiver},
    thread,
};

enum WorkerMessage {
    Log(String),
    Finished(Result<PathBuf, String>),
}

#[derive(Default)]
struct ConverterApp {
    input_path: Option<PathBuf>,
    output_root: Option<PathBuf>,
    logs: Vec<String>,
    status: String,
    result_path: Option<PathBuf>,
    receiver: Option<Receiver<WorkerMessage>>,
    converting: bool,
}

impl ConverterApp {
    fn choose_file(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Roblox files", &["rbxlx", "rbxl", "rbxmx", "rbxm"])
            .pick_file()
        {
            self.input_path = Some(path);
            self.result_path = None;
        }
    }

    fn choose_output(&mut self) {
        if let Some(path) = rfd::FileDialog::new().pick_folder() {
            self.output_root = Some(path);
            self.result_path = None;
        }
    }

    fn can_convert(&self) -> bool {
        self.input_path.is_some() && self.output_root.is_some() && !self.converting
    }

    fn start_conversion(&mut self) {
        let input_path = self.input_path.clone().expect("checked by can_convert");
        let output_root = self.output_root.clone().expect("checked by can_convert");
        let (sender, receiver) = mpsc::channel();

        self.logs.clear();
        self.result_path = None;
        self.status = "Converting".to_string();
        self.converting = true;
        self.receiver = Some(receiver);

        thread::spawn(move || {
            let result = convert_file(&input_path, &output_root, |message| {
                let _ = sender.send(WorkerMessage::Log(message.to_string()));
            })
            .map_err(|error| error.to_string());

            let _ = sender.send(WorkerMessage::Finished(result));
        });
    }

    fn poll_worker(&mut self) {
        if let Some(receiver) = &self.receiver {
            while let Ok(message) = receiver.try_recv() {
                match message {
                    WorkerMessage::Log(message) => self.logs.push(message),
                    WorkerMessage::Finished(Ok(path)) => {
                        self.status = "Done".to_string();
                        self.result_path = Some(path);
                        self.converting = false;
                    }
                    WorkerMessage::Finished(Err(error)) => {
                        self.status = "Failed".to_string();
                        self.logs.push(error);
                        self.converting = false;
                    }
                }
            }
        }
    }

    fn reveal_result(&self) {
        if let Some(path) = &self.result_path {
            #[cfg(target_os = "macos")]
            let _ = Command::new("open").arg("-R").arg(path).status();
        }
    }
}

impl eframe::App for ConverterApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_worker();

        for file in ctx.input(|input| input.raw.dropped_files.clone()) {
            if let Some(path) = file.path {
                self.input_path = Some(path);
                self.result_path = None;
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("rbxlx-to-rojo");
            ui.label("Convert Roblox place and model files into a Rojo project.");
            ui.add_space(12.0);

            ui.group(|ui| {
                ui.label("Input file");
                ui.horizontal(|ui| {
                    let text = self
                        .input_path
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| "Drop a Roblox file here or choose one".to_string());
                    ui.label(text);
                    if ui.button("Choose File").clicked() {
                        self.choose_file();
                    }
                });
            });

            ui.add_space(8.0);

            ui.group(|ui| {
                ui.label("Output folder");
                ui.horizontal(|ui| {
                    let text = self
                        .output_root
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| "Choose where to write the Rojo project".to_string());
                    ui.label(text);
                    if ui.button("Choose Folder").clicked() {
                        self.choose_output();
                    }
                });
            });

            ui.add_space(12.0);

            ui.horizontal(|ui| {
                if ui
                    .add_enabled(self.can_convert(), egui::Button::new("Convert"))
                    .clicked()
                {
                    self.start_conversion();
                }
                ui.label(if self.status.is_empty() {
                    "Waiting"
                } else {
                    &self.status
                });
            });

            if let Some(path) = &self.result_path {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(format!("Created: {}", path.display()));
                    if ui.button("Reveal in Finder").clicked() {
                        self.reveal_result();
                    }
                });
            }

            ui.separator();
            ui.label("Log");
            egui::ScrollArea::vertical().show(ui, |ui| {
                for line in &self.logs {
                    ui.label(line);
                }
            });
        });
    }
}

pub fn run() -> eframe::Result<()> {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "rbxlx-to-rojo",
        options,
        Box::new(|_cc| Ok(Box::<ConverterApp>::default())),
    )
}
```

- [ ] **Step 4: Run GUI build**

Run:

```bash
CARGO_NET_GIT_FETCH_WITH_CLI=true cargo build --features gui --bin rbxlx-to-rojo-gui
```

Expected: build succeeds.

- [ ] **Step 5: Run existing tests**

Run:

```bash
CARGO_NET_GIT_FETCH_WITH_CLI=true cargo test
```

Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/gui.rs src/mac_app.rs src/lib.rs
git commit -m "Add macOS GUI app"
```

---

### Task 3: macOS `.app` Packaging Script

**Files:**
- Create: `scripts/build-mac-app.sh`
- Modify: `.gitignore`

- [ ] **Step 1: Write packaging script**

Create `scripts/build-mac-app.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_NAME="rbxlx-to-rojo"
APP_DIR="$ROOT/dist/$APP_NAME.app"
MACOS_DIR="$APP_DIR/Contents/MacOS"
RESOURCES_DIR="$APP_DIR/Contents/Resources"

cargo build --release --features gui --bin rbxlx-to-rojo-gui

rm -rf "$APP_DIR"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"
cp "$ROOT/target/release/rbxlx-to-rojo-gui" "$MACOS_DIR/$APP_NAME"

cat > "$APP_DIR/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key>
  <string>rbxlx-to-rojo</string>
  <key>CFBundleIdentifier</key>
  <string>com.rbxlx-to-rojo.app</string>
  <key>CFBundleName</key>
  <string>rbxlx-to-rojo</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>1.0.1</string>
  <key>CFBundleVersion</key>
  <string>1</string>
  <key>LSMinimumSystemVersion</key>
  <string>11.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
PLIST

echo "Created $APP_DIR"
```

- [ ] **Step 2: Make the script executable and ignore bundles**

Run:

```bash
chmod +x scripts/build-mac-app.sh
```

Add to `.gitignore`:

```gitignore
dist/
```

- [ ] **Step 3: Run packaging script**

Run:

```bash
CARGO_NET_GIT_FETCH_WITH_CLI=true scripts/build-mac-app.sh
```

Expected: `dist/rbxlx-to-rojo.app/Contents/MacOS/rbxlx-to-rojo` exists.

- [ ] **Step 4: Smoke test binary launch without opening GUI**

Run:

```bash
test -x dist/rbxlx-to-rojo.app/Contents/MacOS/rbxlx-to-rojo
```

Expected: exit code `0`.

- [ ] **Step 5: Commit**

```bash
git add scripts/build-mac-app.sh .gitignore
git commit -m "Add macOS app packaging script"
```

---

### Task 4: Manual macOS Smoke Test

**Files:**
- Modify: `docs/superpowers/plans/2026-05-08-mac-app-wrapper.md` checkboxes only.

- [ ] **Step 1: Launch the app**

Run:

```bash
open dist/rbxlx-to-rojo.app
```

Expected: a single window titled `rbxlx-to-rojo` appears.

- [ ] **Step 2: Convert the provided large file**

Use the app to select or drag:

```text
/Users/guxin/Desktop/test/Place_131756752872026_Dive Down_FULL_06-05-2026_15-25-25.rbxlx
```

Select output folder:

```text
/private/tmp/rbxlx-to-rojo-gui-smoke
```

Click `Convert`.

Expected: status reaches `Done`, log includes `Done`, and a result path is shown.

- [ ] **Step 3: Verify generated files**

Run:

```bash
find "/private/tmp/rbxlx-to-rojo-gui-smoke/Place_131756752872026_Dive Down_FULL_06-05-2026_15-25-25" -type f | wc -l
find "/private/tmp/rbxlx-to-rojo-gui-smoke/Place_131756752872026_Dive Down_FULL_06-05-2026_15-25-25/src" -type f -name '*.lua' | wc -l
```

Expected: file count is non-zero and Lua count is non-zero. Based on the CLI smoke test, expected counts are `597+` generated project files and `429` Lua files.

- [ ] **Step 4: Final verification**

Run:

```bash
CARGO_NET_GIT_FETCH_WITH_CLI=true cargo test
CARGO_NET_GIT_FETCH_WITH_CLI=true cargo build --features gui --bin rbxlx-to-rojo-gui
```

Expected: both commands exit `0`.

- [ ] **Step 5: Commit smoke-test plan checkbox updates only if checkboxes were edited**

```bash
git add docs/superpowers/plans/2026-05-08-mac-app-wrapper.md
git commit -m "Record macOS app smoke test"
```
