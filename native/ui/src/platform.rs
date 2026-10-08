//! Small, shared Linux desktop result classification. Never display child output as an error.
use std::{any::Any, path::Path};

pub const DIALOG_FAILED: &str = "Der Dateidialog konnte nicht gestartet oder abgeschlossen werden. Bitte die Desktop-Sitzung und Zenity/KDialog prüfen oder den Pfad direkt eingeben.";
pub const DIALOG_TIMEOUT: &str = "Der Dateidialog hat nicht rechtzeitig geantwortet. Bitte erneut öffnen oder den Pfad direkt eingeben.";
pub const DIALOG_INVALID: &str =
    "Der Dateidialog hat keinen gültigen absoluten Pfad geliefert. Bitte den Pfad direkt eingeben.";
pub const EXPORT_FAILED: &str = "Der Bericht konnte nicht gespeichert werden. Bitte einen neuen Dateinamen und einen beschreibbaren Ordner wählen.";
pub const WINDOW_FAILED: &str = "Die Verbindung zum Linux-Desktop konnte nicht geöffnet werden. Bitte Flightdeck in der aktuellen X11-/Wayland-Sitzung starten und deren Display-Bibliotheken prüfen.";
pub const RENDERER_FAILED: &str = "Die Softwaredarstellung des Flightdeck-Fensters konnte nicht initialisiert werden. Bitte die X11-/Wayland-Bibliotheken der Desktop-Sitzung prüfen.";
pub const EXECUTOR_FAILED: &str = "Flightdeck konnte seine Hintergrundaufgaben nicht starten. Bitte freie Systemressourcen und Prozesslimits prüfen.";

pub fn selected_path(code: Option<i32>, output: &[u8]) -> Result<Option<String>, &'static str> {
    // Both tools document 1 for cancellation. Some toolkit failures also use
    // that code, so it cannot prove the dialog was successfully displayed.
    if code == Some(1) {
        return Ok(None);
    }
    if code != Some(0) {
        return Err(DIALOG_FAILED);
    }
    if output.len() > 4098 {
        return Err(DIALOG_INVALID);
    }
    let path = std::str::from_utf8(output)
        .map_err(|_| DIALOG_INVALID)?
        .trim_end_matches(['\r', '\n']);
    if path.is_empty()
        || path.len() > 4096
        || !Path::new(path).is_absolute()
        || path.chars().any(char::is_control)
    {
        return Err(DIALOG_INVALID);
    }
    Ok(Some(path.to_owned()))
}

pub fn startup_error(error: &iced::Error) -> &'static str {
    match error {
        iced::Error::ExecutorCreationFailed(_) => EXECUTOR_FAILED,
        iced::Error::WindowCreationFailed(_) => WINDOW_FAILED,
        iced::Error::GraphicsCreationFailed(_) => RENDERER_FAILED,
    }
}

pub fn startup_panic(payload: &(dyn Any + Send)) -> Option<&'static str> {
    let message = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())?;
    // iced_winit 0.14.1 uses expect at these two initialization boundaries.
    // Do not turn arbitrary application panics into a display diagnosis.
    (message.starts_with("Create event loop:") || message.starts_with("Create window:"))
        .then_some(WINDOW_FAILED)
}
