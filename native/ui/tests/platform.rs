//! Desktop boundaries accept only well-formed selections and fixed error classes.
use flightdeck_ui::{App, Edition, Language, Message, platform::*};

#[test]
fn dialog_paths_preserve_filename_spaces_and_reject_malformed_successes() {
    assert_eq!(
        selected_path(Some(0), b"/tmp/report .json \n").expect("path"),
        Some("/tmp/report .json ".into())
    );
    assert_eq!(selected_path(Some(1), b"ignored").expect("cancel"), None);
    for raw in [
        b"".as_slice(),
        b"relative",
        b"/tmp/one\n/tmp/two\n",
        b"/tmp/\0",
        b"/tmp/\xff",
    ] {
        assert_eq!(selected_path(Some(0), raw), Err(DIALOG_INVALID));
    }
    assert_eq!(
        selected_path(Some(0), &vec![b'/'; 4099]),
        Err(DIALOG_INVALID)
    );
    for code in [None, Some(2), Some(127), Some(255)] {
        assert_eq!(selected_path(code, b"/tmp/valid"), Err(DIALOG_FAILED));
    }
}

#[test]
fn startup_diagnosis_does_not_expose_underlying_errors_or_hide_other_panics() {
    let error =
        iced::Error::ExecutorCreationFailed(std::io::Error::other("private underlying detail"));
    assert_eq!(startup_error(&error), EXECUTOR_FAILED);
    let error =
        iced::Error::WindowCreationFailed(Box::new(std::io::Error::other("private display path")));
    assert_eq!(startup_error(&error), WINDOW_FAILED);
    assert_eq!(
        startup_panic(&"Create event loop: private display path"),
        Some(WINDOW_FAILED)
    );
    assert_eq!(
        startup_panic(&String::from("Create window: private display path")),
        Some(WINDOW_FAILED)
    );
    assert_eq!(startup_panic(&"unexpected application bug"), None);
    assert_eq!(startup_panic(&42_u32), None);
}

#[test]
fn export_failure_is_visible_in_both_languages_and_cancellation_is_silent() {
    for language in [Language::De, Language::En] {
        let mut app = App::new(Edition::Msfs2024);
        app.language = language;
        let _ = app.update(Message::Exported(Err(DIALOG_FAILED.into())));
        let notice = app.notice.as_deref().expect("visible error");
        assert!(notice.contains(if language == Language::De {
            "Dateidialog"
        } else {
            "file dialog"
        }));
        let _ = app.update(Message::Dismiss);
        let _ = app.update(Message::Exported(Ok(None)));
        assert!(app.notice.is_none());
    }
}
