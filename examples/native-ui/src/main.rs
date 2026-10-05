//! Uses the production widgets with synthetic state and no service connection.
use flightdeck_ui::{App, Message, settings, theme};
#[path = "../../../native/ui/tests/common/mod.rs"]
mod common;
fn main() -> iced::Result {
    iced::application(
        || common::localized_fixture(flightdeck_ui::Language::De),
        |app: &mut App, message: Message| {
            let task = app.update(message);
            if app.client.is_none() {
                app.notice =
                    Some("Synthetic preview · no game or account actions are executed.".into());
            }
            task
        },
        App::view,
    )
    .title("Flightdeck · Synthetic Rust UI")
    .settings(settings())
    .theme(|_: &App| theme())
    .window(iced::window::Settings {
        size: (1280.0, 900.0).into(),
        min_size: Some((960.0, 700.0).into()),
        ..Default::default()
    })
    .run()
}
