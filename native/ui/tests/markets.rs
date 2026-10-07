use flightdeck_ui::{Action, App, Edition, Language, Message};
use serde_json::json;

mod common;

#[test]
fn initial_store_region_uses_the_backend_suggestion_and_preserves_user_choice() {
    for language in [Language::De, Language::En] {
        let mut app = App::new(Edition::Msfs2024);
        app.startup_checked = true;
        let mut snapshot = common::localized_fixture(language).snapshot;
        snapshot.get_mut("setup").expect("setup")["defaults"]["market"] = json!("US");
        let _ = app.update(Message::Loaded(0, Ok(snapshot.clone())));
        assert_eq!(app.forms.get("market"), "US");
        assert_eq!(
            app.request(&Action::SetupCheck)
                .expect("setup available")
                .body["market"],
            "US"
        );

        let _ = app.update(Message::Field("market", "DE".into()));
        snapshot.get_mut("setup").expect("setup")["defaults"]["market"] = json!("AT");
        let _ = app.update(Message::Loaded(0, Ok(snapshot)));
        assert_eq!(app.forms.get("market"), "DE");
        assert_eq!(
            app.request(&Action::SetupCheck)
                .expect("setup available")
                .body["market"],
            "DE"
        );
    }
}

#[test]
fn missing_region_suggestion_does_not_select_an_unrelated_store() {
    let mut app = App::new(Edition::Msfs2024);
    app.startup_checked = true;
    let mut snapshot = common::localized_fixture(Language::En).snapshot;
    snapshot.get_mut("setup").expect("setup")["defaults"]["market"] = json!("");
    let _ = app.update(Message::Loaded(0, Ok(snapshot)));
    assert!(app.forms.get("market").is_empty());
}
