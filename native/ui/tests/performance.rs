//! Opt-in, offline comparison of page layout and software rendering latency.
mod common;
use flightdeck_ui::{Message, Page, settings, theme};
use iced::{Size, advanced::renderer::Headless};
use iced_test::runtime::{UserInterface, core, user_interface};
use std::time::Instant;

#[test]
#[ignore = "Explicit release-mode UI latency measurement; writes to FLIGHTDECK_UI_BENCHMARK"]
fn measure_page_switches() {
    let settings = settings();
    let mut renderer = iced::futures::executor::block_on(<iced::Renderer as Headless>::new(
        settings.default_font,
        settings.default_text_size,
        Some("tiny-skia"),
    ))
    .expect("renderer");
    let mut app = common::localized_fixture(flightdeck_ui::Language::En);
    let mut cache = user_interface::Cache::default();
    let mut results = Vec::new();
    for scale in [1.0, 2.0] {
        for round in 0..8 {
            for page in [
                Page::Overview,
                Page::Setup,
                Page::Updates,
                Page::Saves,
                Page::Mods,
                Page::Diagnostics,
            ] {
                let start = Instant::now();
                let _ = app.update(Message::Navigate(page));
                let mut ui = UserInterface::build(
                    app.view(),
                    Size::new(1280.0, 900.0),
                    cache,
                    &mut renderer,
                );
                let layout_ms = start.elapsed().as_secs_f64() * 1000.0;
                ui.draw(
                    &mut renderer,
                    &theme(),
                    &core::renderer::Style {
                        text_color: iced::Color::WHITE,
                    },
                    core::mouse::Cursor::Unavailable,
                );
                let rgba = renderer.screenshot(
                    Size::new((1280.0 * scale) as u32, (900.0 * scale) as u32),
                    scale,
                    iced::Color::BLACK,
                );
                std::hint::black_box(&rgba);
                let frame_ms = start.elapsed().as_secs_f64() * 1000.0;
                if round > 0 {
                    results.push(serde_json::json!({"page":format!("{page:?}"),"scale":scale,"layout_ms":layout_ms,"frame_ms":frame_ms}));
                }
                cache = ui.into_cache();
            }
        }
    }
    let result = serde_json::json!({"profile":if cfg!(debug_assertions){"debug"}else{"release"},"scope":"offline synthetic pages; layout plus complete CPU rasterization and pixel readback; excludes network and compositor presentation","samples":results});
    let output = std::env::var("FLIGHTDECK_UI_BENCHMARK").expect("set benchmark output path");
    std::fs::write(output, serde_json::to_vec_pretty(&result).expect("JSON")).expect("result");
}
