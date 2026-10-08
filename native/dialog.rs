//! Bounded native file dialogs. Cancellation is distinct from execution failure.
use crate::{Error, Result, process};
use std::{process::Command, sync::atomic::AtomicBool, time::Duration};

pub fn select(command: &mut Command, timeout: Duration) -> Result<Option<String>> {
    use flightdeck_ui::platform::{DIALOG_FAILED, DIALOG_TIMEOUT, selected_path};
    let (status, output) = process::output_status(command, timeout, 4098, &AtomicBool::new(false))
        .map_err(|error| {
            Error::Invalid(match error {
                Error::Invalid("Der Vorgang hat nicht rechtzeitig geantwortet.") => DIALOG_TIMEOUT,
                _ => DIALOG_FAILED,
            })
        })?;
    selected_path(status.code(), &output).map_err(Error::Invalid)
}
