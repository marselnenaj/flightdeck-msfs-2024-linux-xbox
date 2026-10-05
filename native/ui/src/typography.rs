//! Preserve the web UI's variable weights and letter spacing without rasterizing
//! labels into images. iced's standard Text currently exposes neither setting.
use graphics_text::{Renderer as _, cosmic_text as cosmic};
use iced::advanced::{
    Layout, Widget, graphics::text as graphics_text, layout, mouse, renderer, widget,
};
use iced::{Color, Element, Length, Rectangle, Renderer, Size, Theme};
use std::{
    borrow::Cow,
    sync::{Arc, Once},
};

pub fn load_font(bytes: &'static [u8]) {
    static LOAD: Once = Once::new();
    LOAD.call_once(|| {
        let mut system = graphics_text::font_system()
            .write()
            .expect("font system lock");
        system.load_font(bytes.into());
        // cosmic-text 0.15 matches faces by exact weight before applying the
        // variable wght axis. Register aliases of the same unmodified font data,
        // so 550/650/750 cannot silently fall back to a system font.
        let face = system
            .raw()
            .db()
            .faces()
            .find(|face| face.families.iter().any(|(name, _)| name == "Manrope"))
            .expect("embedded Manrope family")
            .clone();
        for weight in [400, 500, 550, 600, 650, 700, 750, 800] {
            if face.weight.0 != weight {
                let mut alias = face.clone();
                alias.weight = cosmic::Weight(weight);
                system.raw().db_mut().push_face_info(alias);
            }
        }
    });
}

#[derive(Clone, PartialEq)]
struct Key {
    content: String,
    size: f32,
    line_height: f32,
    tracking: f32,
    weight: u16,
    width: f32,
}
#[derive(Default)]
struct State {
    key: Option<Key>,
    buffer: Option<Arc<cosmic::Buffer>>,
    measured: Size,
}

pub struct Label<'a> {
    content: Cow<'a, str>,
    size: f32,
    line_height: f32,
    tracking: f32,
    weight: u16,
    color: Color,
    width: Length,
}

impl<'a> Label<'a> {
    pub fn new(content: impl Into<Cow<'a, str>>, size: f32, weight: u16, color: Color) -> Self {
        Self {
            content: content.into(),
            size,
            line_height: 1.5,
            tracking: 0.0,
            weight,
            color,
            width: Length::Shrink,
        }
    }
    pub fn line_height(mut self, value: f32) -> Self {
        self.line_height = value;
        self
    }
    pub fn tracking(mut self, value: f32) -> Self {
        self.tracking = value;
        self
    }
    pub fn weight(mut self, value: u16) -> Self {
        self.weight = value;
        self
    }
    pub fn width(mut self, value: impl Into<Length>) -> Self {
        self.width = value.into();
        self
    }
}

impl<Message> Widget<Message, Theme, Renderer> for Label<'_> {
    fn size(&self) -> Size<Length> {
        Size::new(self.width, Length::Shrink)
    }
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }
    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State::default())
    }
    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::sized(limits, self.width, Length::Shrink, |limits| {
            let state = tree.state.downcast_mut::<State>();
            let key = Key {
                content: self.content.to_string(),
                size: self.size,
                line_height: self.line_height,
                tracking: self.tracking,
                weight: self.weight,
                width: limits.max().width,
            };
            if state.key.as_ref() != Some(&key) {
                let mut system = graphics_text::font_system()
                    .write()
                    .expect("font system lock");
                let mut buffer = cosmic::Buffer::new(
                    system.raw(),
                    cosmic::Metrics::new(self.size, self.size * self.line_height),
                );
                buffer.set_size(system.raw(), Some(limits.max().width), None);
                buffer.set_wrap(system.raw(), cosmic::Wrap::WordOrGlyph);
                buffer.set_text(
                    system.raw(),
                    &self.content,
                    &cosmic::Attrs::new()
                        .family(cosmic::Family::Name("Manrope"))
                        .weight(cosmic::Weight(self.weight))
                        .letter_spacing(self.tracking / self.size),
                    cosmic::Shaping::Advanced,
                    None,
                );
                buffer.shape_until_scroll(system.raw(), false);
                state.measured = graphics_text::measure(&buffer).0;
                state.buffer = Some(Arc::new(buffer));
                state.key = Some(key);
            }
            state.measured
        })
    }
    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        if let Some(buffer) = &tree.state.downcast_ref::<State>().buffer
            && let Some(clip_bounds) = layout.bounds().intersection(viewport)
        {
            renderer.fill_raw(graphics_text::Raw {
                buffer: Arc::downgrade(buffer),
                position: layout.bounds().position(),
                color: self.color,
                clip_bounds,
            });
        }
    }
    fn operate(
        &mut self,
        _tree: &mut widget::Tree,
        layout: Layout<'_>,
        _renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        operation.text(None, layout.bounds(), &self.content);
    }
}

impl<'a, Message: 'a> From<Label<'a>> for Element<'a, Message> {
    fn from(label: Label<'a>) -> Self {
        Element::new(label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_ui_weight_shapes_using_embedded_manrope() {
        load_font(crate::FONT_BYTES);
        let mut system = graphics_text::font_system()
            .write()
            .expect("font system lock");
        for weight in [400, 550, 600, 650, 700, 750] {
            let mut buffer = cosmic::Buffer::new(system.raw(), cosmic::Metrics::new(24.0, 36.0));
            buffer.set_size(system.raw(), Some(600.0), None);
            buffer.set_text(
                system.raw(),
                "Übersicht · Spielstände 2024",
                &cosmic::Attrs::new()
                    .family(cosmic::Family::Name("Manrope"))
                    .weight(cosmic::Weight(weight)),
                cosmic::Shaping::Advanced,
                None,
            );
            buffer.shape_until_scroll(system.raw(), false);
            let glyphs: Vec<_> = buffer.layout_runs().flat_map(|run| run.glyphs).collect();
            assert!(!glyphs.is_empty());
            for glyph in glyphs {
                let face = system
                    .raw()
                    .db()
                    .face(glyph.font_id)
                    .expect("shaped font exists");
                assert!(
                    face.families.iter().any(|(name, _)| name == "Manrope"),
                    "weight {weight} used {}",
                    face.post_script_name
                );
            }
        }
    }
}
