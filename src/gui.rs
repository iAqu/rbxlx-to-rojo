use eframe::egui;
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
};

const ROBLOX_EXTENSIONS: &[&str] = &["rbxlx", "rbxl", "rbxmx", "rbxm"];

enum WorkerMessage {
    Log(String),
    Finished(Result<PathBuf, String>),
}

pub struct ConverterApp {
    input_path: Option<PathBuf>,
    output_root: Option<PathBuf>,
    logs: Vec<String>,
    status: String,
    result_path: Option<PathBuf>,
    worker: Option<Receiver<WorkerMessage>>,
    converting: bool,
}

impl Default for ConverterApp {
    fn default() -> Self {
        Self {
            input_path: None,
            output_root: None,
            logs: Vec::new(),
            status: "Ready".to_owned(),
            result_path: None,
            worker: None,
            converting: false,
        }
    }
}

impl ConverterApp {
    fn choose_input_file(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Roblox files", ROBLOX_EXTENSIONS)
            .pick_file()
        {
            self.set_input_path(path);
        }
    }

    fn choose_output_folder(&mut self) {
        if let Some(path) = rfd::FileDialog::new().pick_folder() {
            self.output_root = Some(path);
            self.result_path = None;
            self.status = "Ready".to_owned();
        }
    }

    fn set_input_path(&mut self, path: PathBuf) {
        if is_supported_input_file(&path) {
            self.input_path = Some(path);
            self.result_path = None;
            self.status = "Ready".to_owned();
        } else {
            self.input_path = None;
            self.result_path = None;
            self.status = "Unsupported file type".to_owned();
            self.logs
                .push(format!("Unsupported file type: {}", path.display()));
        }
    }

    fn convert(&mut self, ctx: &egui::Context) {
        let input_path = match self.input_path.clone() {
            Some(path) => path,
            None => return,
        };
        let output_root = match self.output_root.clone() {
            Some(path) => path,
            None => return,
        };

        let (sender, receiver) = mpsc::channel();
        let repaint_ctx = ctx.clone();

        self.logs.clear();
        self.result_path = None;
        self.status = "Converting...".to_owned();
        self.worker = Some(receiver);
        self.converting = true;

        thread::spawn(move || {
            let result = crate::converter::convert_file(&input_path, &output_root, |message| {
                let _ = sender.send(WorkerMessage::Log(message.to_owned()));
                repaint_ctx.request_repaint();
            })
            .map_err(|error| error.to_string());

            let _ = sender.send(WorkerMessage::Finished(result));
            repaint_ctx.request_repaint();
        });
    }

    fn drain_worker_messages(&mut self) {
        if let Some(receiver) = self.worker.take() {
            let mut finished = false;

            loop {
                match receiver.try_recv() {
                    Ok(WorkerMessage::Log(message)) => self.logs.push(message),
                    Ok(WorkerMessage::Finished(Ok(path))) => {
                        self.status = "Done".to_owned();
                        self.result_path = Some(path);
                        self.converting = false;
                        finished = true;
                        break;
                    }
                    Ok(WorkerMessage::Finished(Err(error))) => {
                        self.status = "Failed".to_owned();
                        self.logs.push(error);
                        self.result_path = None;
                        self.converting = false;
                        finished = true;
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.status = "Failed".to_owned();
                        self.logs
                            .push("Conversion worker stopped unexpectedly".to_owned());
                        self.result_path = None;
                        self.converting = false;
                        finished = true;
                        break;
                    }
                }
            }

            if !finished {
                self.worker = Some(receiver);
            }
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        if self.converting {
            return;
        }

        let dropped_files = ctx.input(|input| input.raw.dropped_files.clone());
        for file in dropped_files {
            if let Some(path) = file.path {
                self.set_input_path(path);
                break;
            }
        }
    }

    fn reveal_result(&mut self) {
        let path = match &self.result_path {
            Some(path) => path,
            None => return,
        };

        match Command::new("open").arg("-R").arg(path).status() {
            Ok(status) if status.success() => {}
            Ok(status) => self
                .logs
                .push(format!("Reveal in Finder failed with status: {}", status)),
            Err(error) => self
                .logs
                .push(format!("Reveal in Finder failed: {}", error)),
        }
    }
}

impl eframe::App for ConverterApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_worker_messages();
        self.handle_dropped_files(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("rbxlx-to-rojo");
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                ui.label("Input");
                let label = path_label(&self.input_path);
                ui.monospace(label);
                if ui
                    .add_enabled(!self.converting, egui::Button::new("Choose File"))
                    .clicked()
                {
                    self.choose_input_file();
                }
            });

            ui.horizontal(|ui| {
                ui.label("Output");
                let label = path_label(&self.output_root);
                ui.monospace(label);
                if ui
                    .add_enabled(!self.converting, egui::Button::new("Choose Folder"))
                    .clicked()
                {
                    self.choose_output_folder();
                }
            });

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let can_convert =
                    self.input_path.is_some() && self.output_root.is_some() && !self.converting;
                if ui
                    .add_enabled(can_convert, egui::Button::new("Convert"))
                    .clicked()
                {
                    self.convert(ctx);
                }

                if ui
                    .add_enabled(
                        self.result_path.is_some(),
                        egui::Button::new("Reveal in Finder"),
                    )
                    .clicked()
                {
                    self.reveal_result();
                }

                ui.label(format!("Status: {}", self.status));
            });

            if let Some(path) = &self.result_path {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("Generated project");
                    ui.monospace(path.display().to_string());
                });
            }

            ui.add_space(8.0);
            ui.label("Drop a Roblox place or model file anywhere in this window.");
            ui.separator();
            ui.label("Log");
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    if self.logs.is_empty() {
                        ui.label("No messages yet.");
                    } else {
                        for message in &self.logs {
                            ui.label(message);
                        }
                    }
                });
        });
    }
}

pub fn run() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([720.0, 520.0]),
        ..Default::default()
    };

    eframe::run_native(
        "rbxlx-to-rojo",
        options,
        Box::new(|_cc| Ok(Box::<ConverterApp>::default())),
    )
}

fn path_label(path: &Option<PathBuf>) -> String {
    path.as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "Not selected".to_owned())
}

fn is_supported_input_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            ROBLOX_EXTENSIONS
                .iter()
                .any(|supported| extension.eq_ignore_ascii_case(supported))
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_supported_roblox_file_extensions() {
        assert!(is_supported_input_file(Path::new("place.rbxlx")));
        assert!(is_supported_input_file(Path::new("place.rbxl")));
        assert!(is_supported_input_file(Path::new("model.rbxmx")));
        assert!(is_supported_input_file(Path::new("model.rbxm")));
    }

    #[test]
    fn rejects_unsupported_input_file_extensions() {
        assert!(!is_supported_input_file(Path::new("notes.txt")));
        assert!(!is_supported_input_file(Path::new("rbxlx")));
    }

    #[test]
    fn unsupported_input_clears_previous_selection_and_result() {
        let mut app = ConverterApp::default();
        app.input_path = Some(PathBuf::from("place.rbxlx"));
        app.result_path = Some(PathBuf::from("out/place"));

        app.set_input_path(PathBuf::from("notes.txt"));

        assert!(app.input_path.is_none());
        assert!(app.result_path.is_none());
        assert_eq!(app.status, "Unsupported file type");
    }
}
