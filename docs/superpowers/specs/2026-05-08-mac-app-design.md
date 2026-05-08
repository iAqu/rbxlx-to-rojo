# macOS App Wrapper Design

## Goal

Build a small macOS app for `rbxlx-to-rojo` so users can convert Roblox place/model files without using Terminal. The app should feel like a focused utility: choose or drag in a file, choose an output folder, run conversion, and inspect the result.

## Scope

The app supports `.rbxlx`, `.rbxl`, `.rbxmx`, and `.rbxm` files. It keeps the current CLI available and shares the same conversion implementation with the CLI. The first version does not include project history, settings sync, automatic Rojo installation, or code signing/notarization automation.

## UI

The app uses a single window:

- Input file area with drag-and-drop plus a "Choose File" button.
- Output folder area with a "Choose Folder" button.
- Convert button that is disabled until both paths are valid.
- Status line for current phase: waiting, decoding, converting, done, or failed.
- Log panel showing user-readable progress and errors.
- Result area with the generated project path and a "Reveal in Finder" action after success.

## Architecture

Extract the current CLI conversion flow into a library API, for example `convert_file(input_path, output_root, reporter)`. The API handles extension detection, Roblox DOM decoding, filesystem output creation, and progress reporting. The existing CLI becomes a thin wrapper around this API.

Add a second binary for the Mac app using Rust `eframe/egui`. This avoids introducing a webview stack and keeps the project mostly Rust. The UI calls the shared conversion API on a background thread so the window stays responsive while large `.rbxlx` files decode.

## Error Handling

Errors should be shown in the log panel and preserve the specific decode or IO reason. Invalid file extensions and missing output folders should be caught before conversion starts. If conversion fails partway through, the app should stop cleanly and leave the log visible for copying/debugging.

## Packaging

Add a macOS packaging script that builds the GUI binary in release mode and creates a basic `.app` bundle with `Info.plist`, `MacOS/`, and `Resources/`. The first version can be unsigned for local use.

## Testing

Keep the existing conversion tests for shared behavior. Add focused unit tests for the shared conversion API where practical. Verify the GUI binary builds on macOS and manually smoke test selecting a file, dragging a file, running conversion, and opening the result in Finder.
