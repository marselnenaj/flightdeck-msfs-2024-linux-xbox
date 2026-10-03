// SPDX-License-Identifier: MIT
//! Small bounded XML editor which preserves unrelated settings and launch entries.
use crate::{Error, Result, error::require, game_package};
use quick_xml::{
    Reader, Writer,
    events::{BytesStart, BytesText, Event},
};
const INVALID: &str = "Unsupported or invalid XML settings.";
#[derive(Clone)]
pub enum Item {
    Element(Element),
    Event(Event<'static>),
}
#[derive(Clone)]
pub struct Element {
    pub start: BytesStart<'static>,
    pub items: Vec<Item>,
}
impl Element {
    pub fn new(name: &str) -> Self {
        Self {
            start: BytesStart::new(name.to_string()),
            items: Vec::new(),
        }
    }
    pub fn named(&self, name: &str) -> bool {
        self.start.name().as_ref() == name
    }
    pub fn text(&self) -> String {
        self.items
            .iter()
            .filter_map(|v| match v {
                Item::Event(Event::Text(v)) => Some(
                    quick_xml::escape::unescape(v.as_ref())
                        .map(|v| v.into_owned())
                        .unwrap_or_default(),
                ),
                Item::Event(Event::CData(v)) => Some(v.as_ref().to_string()),
                Item::Event(Event::GeneralRef(v)) => v
                    .resolve_char_ref()
                    .ok()
                    .flatten()
                    .map(|v| v.to_string())
                    .or_else(|| {
                        quick_xml::escape::resolve_predefined_entity(v.as_ref()).map(str::to_string)
                    }),
                _ => None,
            })
            .collect()
    }
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.items.iter().find_map(|v| match v {
            Item::Element(v) if v.named(name) => Some(v),
            _ => None,
        })
    }
    pub fn set(&mut self, name: &str, value: &str) {
        let mut found = false;
        self.items.retain(|item| {
            if matches!(item,Item::Element(v) if v.named(name)) {
                if found {
                    return false;
                }
                found = true;
            }
            true
        });
        if !found {
            self.items.push(Item::Element(Self::new(name)));
        }
        if let Some(Item::Element(node)) = self
            .items
            .iter_mut()
            .find(|v| matches!(v,Item::Element(e) if e.named(name)))
        {
            node.items = vec![Item::Event(Event::Text(BytesText::new(value).into_owned()))];
        }
    }
    fn emit(&self, writer: &mut Writer<Vec<u8>>) -> Result<()> {
        writer.write_event(Event::Start(self.start.clone()))?;
        for item in &self.items {
            match item {
                Item::Element(v) => v.emit(writer)?,
                Item::Event(v) => writer.write_event(v.clone())?,
            }
        }
        writer.write_event(Event::End(self.start.to_end()))?;
        Ok(())
    }
    pub fn bytes(&self) -> Result<Vec<u8>> {
        let mut writer = Writer::new(b"<?xml version=\"1.0\" encoding=\"utf-8\"?>\n".to_vec());
        self.emit(&mut writer)?;
        Ok(writer.into_inner())
    }
}
pub fn parse(bytes: &[u8]) -> Result<Element> {
    require(bytes.len() <= 2 * 1024 * 1024, INVALID)?;
    let text = game_package::text(bytes)?;
    let mut reader = Reader::from_str(&text);
    let mut stack: Vec<Element> = Vec::new();
    let mut root = None;
    let mut count = 0;
    loop {
        count += 1;
        require(count <= 100000 && stack.len() <= 128, INVALID)?;
        let event = reader.read_event().map_err(|_| Error::Invalid(INVALID))?;
        match event {
            Event::Start(v) => {
                require(root.is_none(), INVALID)?;
                for attr in v.attributes() {
                    attr.map_err(|_| Error::Invalid(INVALID))?;
                }
                stack.push(Element {
                    start: v.into_owned(),
                    items: Vec::new(),
                });
            }
            Event::Empty(v) => {
                for attr in v.attributes() {
                    attr.map_err(|_| Error::Invalid(INVALID))?;
                }
                let node = Element {
                    start: v.into_owned(),
                    items: Vec::new(),
                };
                if let Some(parent) = stack.last_mut() {
                    parent.items.push(Item::Element(node));
                } else {
                    require(root.is_none(), INVALID)?;
                    root = Some(node);
                }
            }
            Event::End(_) => {
                let node = stack.pop().ok_or(Error::Invalid(INVALID))?;
                if let Some(parent) = stack.last_mut() {
                    parent.items.push(Item::Element(node));
                } else {
                    root = Some(node);
                }
            }
            Event::DocType(_) => return Err(Error::Invalid(INVALID)),
            Event::GeneralRef(v) => {
                require(
                    v.resolve_char_ref()
                        .map_err(|_| Error::Invalid(INVALID))?
                        .is_some()
                        || quick_xml::escape::resolve_predefined_entity(v.as_ref()).is_some(),
                    INVALID,
                )?;
                stack
                    .last_mut()
                    .ok_or(Error::Invalid(INVALID))?
                    .items
                    .push(Item::Event(Event::GeneralRef(v.into_owned())));
            }
            Event::Eof => {
                require(stack.is_empty(), INVALID)?;
                break;
            }
            Event::Decl(_) => {
                require(stack.is_empty() && root.is_none(), INVALID)?;
            }
            other => {
                if let Some(parent) = stack.last_mut() {
                    parent.items.push(Item::Event(other.into_owned()));
                } else if let Event::Text(v) = other {
                    require(v.as_ref().trim().is_empty(), INVALID)?;
                }
            }
        }
    }
    root.ok_or(Error::Invalid(INVALID))
}
