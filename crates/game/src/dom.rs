use wasm_bindgen::prelude::*;
use web_sys::{Document, Element, HtmlElement, HtmlInputElement};

pub fn document() -> Document {
    web_sys::window()
        .and_then(|window| window.document())
        .expect("document")
}

pub fn element(tag: &str, class: &str) -> HtmlElement {
    let element: HtmlElement = document()
        .create_element(tag)
        .expect("create element")
        .unchecked_into();
    if !class.is_empty() {
        element.set_class_name(class);
    }
    element
}

pub fn text(tag: &str, class: &str, content: &str) -> HtmlElement {
    let element = element(tag, class);
    element.set_text_content(Some(content));
    element
}

pub fn input(kind: &str, placeholder: &str) -> HtmlInputElement {
    let input: HtmlInputElement = element("input", "field").unchecked_into();
    input.set_type(kind);
    input.set_placeholder(placeholder);
    input
}

pub fn append(parent: &Element, child: &Element) {
    let _ = parent.append_child(child);
}

pub fn clear(element: &Element) {
    element.set_inner_html("");
}

pub struct Listener {
    target: Element,
    event: String,
    closure: Closure<dyn Fn(web_sys::Event)>,
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = self.target.remove_event_listener_with_callback(
            &self.event,
            self.closure.as_ref().unchecked_ref(),
        );
    }
}

pub fn listen(
    element: &Element,
    event: &str,
    handler: impl Fn(web_sys::Event) + 'static,
) -> Listener {
    let closure = Closure::<dyn Fn(web_sys::Event)>::new(handler);
    let _ = element.add_event_listener_with_callback(event, closure.as_ref().unchecked_ref());
    Listener {
        target: element.clone(),
        event: event.to_owned(),
        closure,
    }
}

pub fn on(element: &Element, event: &str, handler: impl Fn(web_sys::Event) + 'static) {
    std::mem::forget(listen(element, event, handler));
}

pub fn show(element: &HtmlElement, visible: bool) {
    let _ = element
        .style()
        .set_property("display", if visible { "" } else { "none" });
}

pub fn set_text(element: &HtmlElement, value: &str) {
    if element.text_content().as_deref() != Some(value) {
        element.set_text_content(Some(value));
    }
}

pub fn set_style(element: &HtmlElement, property: &str, value: &str) {
    let style = element.style();
    if style.get_property_value(property).ok().as_deref() != Some(value) {
        let _ = style.set_property(property, value);
    }
}

pub fn set_class(element: &HtmlElement, class: &str, enabled: bool) {
    let list = element.class_list();
    if list.contains(class) != enabled {
        let _ = list.toggle_with_force(class, enabled);
    }
}

pub fn rich_markup(raw: &str) -> String {
    let mut output = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find('<') {
        output.push_str(&escape(&rest[..start]));
        let Some(end) = rest[start..].find('>') else {
            rest = "";
            break;
        };
        let tag = &rest[start + 1..start + end];
        let name: String = tag
            .trim_start_matches('/')
            .chars()
            .take_while(|character| character.is_ascii_alphanumeric())
            .collect();
        if name.eq_ignore_ascii_case("br") {
            output.push_str("<br>");
        } else if tag.starts_with('/') {
            output.push_str("</span>");
        } else if !tag.ends_with('/') && !name.is_empty() {
            output.push_str(&format!("<span class=\"t-{}\">", name.to_lowercase()));
        }
        rest = &rest[start + end + 1..];
    }
    output.push_str(&escape(rest));
    output
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_tags_become_safe_spans() {
        assert_eq!(
            rich_markup("<stats><attention>75</attention> AD</stats><br>x"),
            "<span class=\"t-stats\"><span class=\"t-attention\">75</span> AD</span><br>x"
        );
        assert_eq!(
            rich_markup("<img src=x onerror=alert(1)>hi"),
            "<span class=\"t-img\">hi"
        );
        assert_eq!(rich_markup("a & b"), "a &amp; b");
    }
}
