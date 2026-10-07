//! Software-renderer regressions for disabled controls and cached page/size changes.
mod common;
use flightdeck_ui::{Action, App, Language, Message, Page, settings, theme};
use iced::{Color, Point, Renderer, Size, advanced::renderer::Headless};
use iced_test::runtime::{UserInterface, core, user_interface};
use serde_json::json;

fn renderer() -> Renderer {
    let settings = settings();
    iced::futures::executor::block_on(<Renderer as Headless>::new(
        settings.default_font,
        settings.default_text_size,
        Some("tiny-skia"),
    ))
    .expect("software renderer")
}

fn render(
    app: &App,
    size: Size,
    scale: f32,
    renderer: &mut Renderer,
    cache: user_interface::Cache,
) -> (Vec<u8>, user_interface::Cache) {
    let mut ui = UserInterface::build(app.view(), size, cache, renderer);
    ui.update(
        &[iced::Event::Window(iced::window::Event::RedrawRequested(
            std::time::Instant::now(),
        ))],
        core::mouse::Cursor::Unavailable,
        renderer,
        &mut core::clipboard::Null,
        &mut Vec::new(),
    );
    ui.draw(
        renderer,
        &theme(),
        &core::renderer::Style {
            text_color: Color::WHITE,
        },
        core::mouse::Cursor::Unavailable,
    );
    let pixels = renderer.screenshot(
        Size::new((size.width * scale) as u32, (size.height * scale) as u32),
        scale,
        Color::BLACK,
    );
    (pixels, ui.into_cache())
}

#[test]
fn blocked_launch_is_visually_disabled_and_does_not_show_ready() {
    let size = Size::new(1061.0, 1125.0);
    let mut app = common::localized_fixture(Language::En);
    let ready = render(&app, size, 1.0, &mut renderer(), Default::default()).0;
    app.snapshot.get_mut("status").expect("status")["game"]["can_start"] = json!(false);
    app.snapshot.get_mut("status").expect("status")["cloud"] = json!({"state":"attention","enabled":true,"can_retry":true,"request_id":"interrupted-session","error_code":"unsafe_session","message":"Previous session needs recovery."});
    let mut ui = iced_test::Simulator::with_size(settings(), size, app.view());
    ui.find("Action needed")
        .expect("attention is visible in the header");
    assert!(ui.find("Flightdeck ready").is_err());
    let bounds = ui.find(app.launch_label()).expect("launch label").bounds();
    let sample = Point::new(bounds.center_x(), bounds.y + bounds.height + 8.0);
    let blocked = render(&app, size, 1.0, &mut renderer(), Default::default()).0;
    let index = (sample.y as usize * size.width as usize + sample.x as usize) * 4;
    if let Ok(directory) = std::env::var("FLIGHTDECK_UI_RENDER_CAPTURE") {
        let directory = std::path::Path::new(&directory);
        std::fs::create_dir_all(directory).expect("capture directory");
        for (name, pixels) in [("ready", &ready), ("attention", &blocked)] {
            image::save_buffer(
                directory.join(format!("{name}.png")),
                pixels,
                size.width as u32,
                size.height as u32,
                image::ColorType::Rgba8,
            )
            .expect("capture image");
        }
    }
    assert!(
        blocked[index + 1] < ready[index + 1] / 2,
        "blocked button retained active cyan at {sample:?}: ready {:?}, blocked {:?}",
        &ready[index..index + 4],
        &blocked[index..index + 4]
    );
}

#[test]
fn recovery_is_visible_and_only_emits_the_existing_retry_action() {
    let mut app = common::localized_fixture(Language::En);
    app.snapshot.get_mut("status").expect("status")["game"]["can_start"] = json!(false);
    app.snapshot.get_mut("status").expect("status")["cloud"] = json!({"state":"attention","enabled":true,"can_retry":true,"request_id":"interrupted-session","error_code":"unsafe_session"});
    for size in [Size::new(750.0, 900.0), Size::new(1061.0, 1125.0)] {
        let mut ui = iced_test::Simulator::with_size(settings(), size, app.view());
        let bounds = ui
            .click("Check session")
            .expect("visible recovery action")
            .bounds();
        assert!(bounds.y + bounds.height < size.height);
        assert!(matches!(
            ui.into_messages().next(),
            Some(Message::Action(Action::Automatic("retry")))
        ));
    }
    app.pending = true;
    assert!(app.launch_message().is_none());
    assert!(
        app.request(&Action::Select(flightdeck_ui::Edition::Msfs2020))
            .is_none()
    );
    let mut ui = iced_test::Simulator::with_size(settings(), Size::new(1061.0, 1125.0), app.view());
    assert!(ui.find("Check session").is_err());
    drop(ui);
    app.pending = false;
    app.snapshot.get_mut("status").expect("status")["cloud"]["state"] = json!("syncing");
    app.snapshot.get_mut("status").expect("status")["cloud"]["phase"] = json!("recovery");
    assert_eq!(app.launch_label(), "Checking session …");
    assert!(app.launch_message().is_none());
    assert!(
        app.request(&Action::Select(flightdeck_ui::Edition::Msfs2020))
            .is_none()
    );
    let mut ui = iced_test::Simulator::with_size(settings(), Size::new(1061.0, 1125.0), app.view());
    assert!(ui.find("Check session").is_err());
}

#[test]
fn failed_session_is_visible_but_start_remains_retryable() {
    for language in [Language::De, Language::En] {
        let mut app = common::localized_fixture(language);
        app.snapshot.get_mut("status").expect("status")["game"]["exit_code"] = json!(1);
        app.snapshot.get_mut("status").expect("status")["cloud"]["state"] = json!("done");
        app.snapshot.get_mut("status").expect("status")["cloud"]["message"] =
            json!("Saves synchronized.");
        let (state, retry, reason) = if language == Language::De {
            (
                "Simulator-Sitzung fehlgeschlagen",
                "Simulator starten",
                "du kannst erneut starten",
            )
        } else {
            (
                "Simulator session failed",
                "Start simulator",
                "you can try starting again",
            )
        };
        assert_eq!(app.launch_state(), state);
        assert!(app.launch_note().contains(reason));
        let mut ui =
            iced_test::Simulator::with_size(settings(), Size::new(1061.0, 1125.0), app.view());
        ui.find(state).expect("session failure is visible");
        ui.find(app.launch_note())
            .expect("failure guidance is visible");
        ui.click(retry).expect("retry remains enabled");
        assert!(matches!(
            ui.into_messages().next(),
            Some(Message::Action(Action::Launch))
        ));
        for code in [0, 130, 143] {
            app.snapshot.get_mut("status").expect("status")["game"]["exit_code"] = json!(code);
            assert_ne!(app.launch_state(), state);
            assert!(!app.launch_note().contains(reason));
        }
        app.snapshot.get_mut("status").expect("status")["game"]["exit_code"] = json!(1);
        app.snapshot.get_mut("status").expect("status")["cloud"]["state"] = json!("syncing");
        assert_ne!(app.launch_state(), state);
    }
}

#[test]
fn cached_page_and_size_changes_match_a_fresh_full_render() {
    let mut app = common::localized_fixture(Language::En);
    let mut renderer = renderer();
    let mut cache = user_interface::Cache::default();
    for (page, width, height, scale) in [
        (Page::Overview, 1280.0, 900.0, 1.0),
        (Page::Setup, 750.0, 900.0, 1.5),
        (Page::Overview, 1061.0, 1125.0, 1.0),
        (Page::Mods, 960.0, 700.0, 2.0),
        (Page::Overview, 1280.0, 900.0, 1.0),
    ] {
        app.page = page;
        let size = Size::new(width, height);
        let (actual, next_cache) = render(&app, size, scale, &mut renderer, cache);
        cache = next_cache;
        let mut fresh = self::renderer();
        let expected = render(&app, size, scale, &mut fresh, Default::default()).0;
        assert_eq!(
            actual, expected,
            "cached render differs for {page:?} at {width}×{height}, scale {scale}"
        );
    }
}
