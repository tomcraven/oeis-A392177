//! Browser clipboard helpers for WASM (itch embeds, mobile WebKit, cross-origin iframes).
//!
//! Copy uses a hidden DOM `<textarea>` during the click handler so `execCommand('copy')` keeps the user
//! gesture; paste opens a real textarea dialog because `navigator.clipboard.readText()` is blocked in
//! itch.io iframes even when the site has clipboard permission.

use bevy::prelude::*;
use crossbeam_channel::{Receiver, Sender, unbounded};
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

use crate::ui::UiState;

const TEXTAREA_ID: &str = "rbk-share-code-copy";
const COPY_MODAL_ID: &str = "rbk-share-code-modal";
const PASTE_MODAL_ID: &str = "rbk-share-code-paste-modal";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShareCodeCopyOutcome {
    /// Clipboard updated during the button click.
    SystemClipboard,
    /// Automatic copy failed; a dismissible dialog shows the code for manual copy.
    ManualCopyDialog,
}

/// Publishes `text` for copying during a user gesture (e.g. egui button click).
pub fn publish_share_code_for_copy(text: &str) -> Result<ShareCodeCopyOutcome, String> {
    let window = web_sys::window().ok_or("no browser window")?;
    let document = window.document().ok_or("no document")?;
    let body = document.body().ok_or("no document body")?;

    dismiss_copy_modal(&document);

    let textarea = get_or_create_hidden_textarea(&document, &body)?;
    textarea.set_value(text);
    textarea.set_read_only(true);

    let len = text.chars().count() as u32;
    textarea
        .set_selection_range(0, len)
        .map_err(|_| "setSelectionRange failed")?;
    let _ = textarea.focus();

    let copied = document
        .dyn_ref::<web_sys::HtmlDocument>()
        .and_then(|html_doc| html_doc.exec_command("copy").ok())
        .unwrap_or(false);

    if copied {
        let _ = textarea.blur();
        Ok(ShareCodeCopyOutcome::SystemClipboard)
    } else {
        show_copy_modal(&document, &body, text)?;
        Ok(ShareCodeCopyOutcome::ManualCopyDialog)
    }
}

fn get_or_create_hidden_textarea(
    document: &web_sys::Document,
    body: &web_sys::HtmlElement,
) -> Result<web_sys::HtmlTextAreaElement, String> {
    if let Some(existing) = document.get_element_by_id(TEXTAREA_ID) {
        return existing
            .dyn_into::<web_sys::HtmlTextAreaElement>()
            .map_err(|_| "share copy element was not a textarea".into());
    }

    let textarea = document
        .create_element("textarea")
        .map_err(|_| "failed to create textarea")?
        .dyn_into::<web_sys::HtmlTextAreaElement>()
        .map_err(|_| "new element was not a textarea")?;
    textarea.set_id(TEXTAREA_ID);
    textarea
        .set_attribute("aria-hidden", "true")
        .map_err(|_| "aria-hidden")?;
    textarea
        .set_attribute("tabindex", "-1")
        .map_err(|_| "tabindex")?;
    textarea
        .set_attribute("autocomplete", "off")
        .map_err(|_| "autocomplete")?;
    textarea
        .set_attribute("spellcheck", "false")
        .map_err(|_| "spellcheck")?;

    apply_hidden_textarea_style(&textarea.style())?;
    body.append_child(&textarea)
        .map_err(|_| "failed to mount share copy textarea")?;
    Ok(textarea)
}

fn apply_hidden_textarea_style(style: &web_sys::CssStyleDeclaration) -> Result<(), String> {
    let set = |prop: &str, value: &str| {
        style
            .set_property(prop, value)
            .map_err(|_| format!("style {prop}"))
    };
    set("position", "fixed")?;
    set("left", "-9999px")?;
    set("top", "0")?;
    set("width", "1px")?;
    set("height", "1px")?;
    set("opacity", "0")?;
    set("pointer-events", "none")?;
    Ok(())
}

fn show_copy_modal(
    document: &web_sys::Document,
    body: &web_sys::HtmlElement,
    text: &str,
) -> Result<(), String> {
    dismiss_copy_modal(document);

    let overlay = document
        .create_element("div")
        .map_err(|_| "failed to create modal")?
        .dyn_into::<web_sys::HtmlElement>()
        .map_err(|_| "modal overlay was not an element")?;
    overlay.set_id(COPY_MODAL_ID);

    let overlay_style = overlay.style();
    let set = |prop: &str, value: &str| {
        overlay_style
            .set_property(prop, value)
            .map_err(|_| format!("overlay style {prop}"))
    };
    set("position", "fixed")?;
    set("inset", "0")?;
    set("z-index", "2000")?;
    set("display", "flex")?;
    set("align-items", "center")?;
    set("justify-content", "center")?;
    set("padding", "16px")?;
    set("background", "rgba(0, 0, 0, 0.65)")?;
    set("box-sizing", "border-box")?;

    let panel = document
        .create_element("div")
        .map_err(|_| "failed to create modal panel")?
        .dyn_into::<web_sys::HtmlElement>()
        .map_err(|_| "modal panel was not an element")?;
    let panel_style = panel.style();
    let set_panel = |prop: &str, value: &str| {
        panel_style
            .set_property(prop, value)
            .map_err(|_| format!("panel style {prop}"))
    };
    set_panel("display", "flex")?;
    set_panel("flex-direction", "column")?;
    set_panel("gap", "10px")?;
    set_panel("width", "100%")?;
    set_panel("max-width", "520px")?;
    set_panel("max-height", "min(70vh, 420px)")?;
    set_panel("padding", "14px")?;
    set_panel("border-radius", "8px")?;
    set_panel("background", "#12121a")?;
    set_panel("border", "1px solid #555")?;
    set_panel("box-sizing", "border-box")?;
    set_panel("font-family", "system-ui, sans-serif")?;

    let hint = document
        .create_element("p")
        .map_err(|_| "failed to create hint")?
        .dyn_into::<web_sys::HtmlElement>()
        .map_err(|_| "hint was not an element")?;
    hint.set_text_content(Some(
        "Could not copy automatically. Select the code below, then use Copy from the menu.",
    ));
    let hint_style = hint.style();
    let _ = hint_style.set_property("margin", "0");
    let _ = hint_style.set_property("font-size", "13px");
    let _ = hint_style.set_property("line-height", "1.4");
    let _ = hint_style.set_property("color", "#c8c8c8");

    let field = document
        .create_element("textarea")
        .map_err(|_| "failed to create modal textarea")?
        .dyn_into::<web_sys::HtmlTextAreaElement>()
        .map_err(|_| "modal field was not a textarea")?;
    field.set_value(text);
    field.set_read_only(true);
    let field_style = field.style();
    let set_field = |prop: &str, value: &str| {
        field_style
            .set_property(prop, value)
            .map_err(|_| format!("field style {prop}"))
    };
    set_field("flex", "1 1 auto")?;
    set_field("min-height", "120px")?;
    set_field("width", "100%")?;
    set_field("resize", "none")?;
    set_field("box-sizing", "border-box")?;
    set_field("font-family", "ui-monospace, monospace")?;
    set_field("font-size", "11px")?;
    set_field("line-height", "1.35")?;
    set_field("color", "#e0e0e0")?;
    set_field("background", "#0a0a10")?;
    set_field("border", "1px solid #444")?;
    set_field("border-radius", "4px")?;
    set_field("padding", "8px")?;

    let close = document
        .create_element("button")
        .map_err(|_| "failed to create close button")?
        .dyn_into::<web_sys::HtmlElement>()
        .map_err(|_| "close was not an element")?;
    close.set_text_content(Some("Close"));
    close.set_attribute("type", "button")
        .map_err(|_| "button type")?;
    let close_style = close.style();
    let _ = close_style.set_property("align-self", "flex-end");
    let _ = close_style.set_property("padding", "6px 14px");
    let _ = close_style.set_property("font-size", "13px");
    let _ = close_style.set_property("border-radius", "4px");
    let _ = close_style.set_property("border", "1px solid #555");
    let _ = close_style.set_property("background", "#222");
    let _ = close_style.set_property("color", "#eee");

    panel
        .append_child(&hint)
        .map_err(|_| "append hint")?;
    panel.append_child(&field).map_err(|_| "append field")?;
    panel
        .append_child(&close)
        .map_err(|_| "append close")?;
    overlay
        .append_child(&panel)
        .map_err(|_| "append panel")?;
    body.append_child(&overlay)
        .map_err(|_| "append modal")?;

    let len = text.chars().count() as u32;
    let _ = field.set_selection_range(0, len);
    let _ = field.focus();

    wire_modal_dismiss(document, &overlay, &close)?;

    Ok(())
}

fn wire_modal_dismiss(
    document: &web_sys::Document,
    overlay: &web_sys::HtmlElement,
    close: &web_sys::HtmlElement,
) -> Result<(), String> {
    let doc_for_close = document.clone();
    let overlay_for_close = overlay.clone();
    let on_close = Closure::wrap(Box::new(move |_event: web_sys::MouseEvent| {
        dismiss_copy_modal(&doc_for_close);
        let _ = overlay_for_close;
    }) as Box<dyn FnMut(_)>);
    close
        .add_event_listener_with_callback("click", on_close.as_ref().unchecked_ref())
        .map_err(|_| "close listener")?;
    on_close.forget();

    let doc_for_backdrop = document.clone();
    let overlay_for_backdrop = overlay.clone();
    let on_backdrop = Closure::wrap(Box::new(move |event: web_sys::MouseEvent| {
        if event.target() == Some(overlay_for_backdrop.clone().unchecked_into()) {
            dismiss_copy_modal(&doc_for_backdrop);
        }
    }) as Box<dyn FnMut(_)>);
    overlay
        .add_event_listener_with_callback("click", on_backdrop.as_ref().unchecked_ref())
        .map_err(|_| "backdrop listener")?;
    on_backdrop.forget();

    Ok(())
}

fn dismiss_copy_modal(document: &web_sys::Document) {
    if let Some(modal) = document.get_element_by_id(COPY_MODAL_ID) {
        let _ = modal.remove();
    }
}

fn dismiss_paste_modal(document: &web_sys::Document) {
    if let Some(modal) = document.get_element_by_id(PASTE_MODAL_ID) {
        let _ = modal.remove();
    }
}

/// Opens a real DOM textarea so paste works on itch.io and other cross-origin embeds (no `clipboard-read`).
fn show_paste_modal(
    document: &web_sys::Document,
    body: &web_sys::HtmlElement,
    initial: &str,
    sender: Sender<Result<String, String>>,
) -> Result<(), String> {
    dismiss_paste_modal(document);
    dismiss_copy_modal(document);

    let overlay = document
        .create_element("div")
        .map_err(|_| "failed to create paste modal")?
        .dyn_into::<web_sys::HtmlElement>()
        .map_err(|_| "paste overlay was not an element")?;
    overlay.set_id(PASTE_MODAL_ID);

    let overlay_style = overlay.style();
    let set = |prop: &str, value: &str| {
        overlay_style
            .set_property(prop, value)
            .map_err(|_| format!("overlay style {prop}"))
    };
    set("position", "fixed")?;
    set("inset", "0")?;
    set("z-index", "2000")?;
    set("display", "flex")?;
    set("align-items", "center")?;
    set("justify-content", "center")?;
    set("padding", "16px")?;
    set("background", "rgba(0, 0, 0, 0.65)")?;
    set("box-sizing", "border-box")?;

    let panel = document
        .create_element("div")
        .map_err(|_| "failed to create paste panel")?
        .dyn_into::<web_sys::HtmlElement>()
        .map_err(|_| "paste panel was not an element")?;
    let panel_style = panel.style();
    let set_panel = |prop: &str, value: &str| {
        panel_style
            .set_property(prop, value)
            .map_err(|_| format!("panel style {prop}"))
    };
    set_panel("display", "flex")?;
    set_panel("flex-direction", "column")?;
    set_panel("gap", "10px")?;
    set_panel("width", "100%")?;
    set_panel("max-width", "520px")?;
    set_panel("max-height", "min(70vh, 420px)")?;
    set_panel("padding", "14px")?;
    set_panel("border-radius", "8px")?;
    set_panel("background", "#12121a")?;
    set_panel("border", "1px solid #555")?;
    set_panel("box-sizing", "border-box")?;
    set_panel("font-family", "system-ui, sans-serif")?;

    let field = document
        .create_element("textarea")
        .map_err(|_| "failed to create paste field")?
        .dyn_into::<web_sys::HtmlTextAreaElement>()
        .map_err(|_| "paste field was not a textarea")?;
    field.set_value(initial);
    field
        .set_attribute("spellcheck", "false")
        .map_err(|_| "spellcheck")?;
    field
        .set_attribute("autocomplete", "off")
        .map_err(|_| "autocomplete")?;
    let field_style = field.style();
    let set_field = |prop: &str, value: &str| {
        field_style
            .set_property(prop, value)
            .map_err(|_| format!("field style {prop}"))
    };
    set_field("flex", "1 1 auto")?;
    set_field("min-height", "88px")?;
    set_field("width", "100%")?;
    set_field("resize", "vertical")?;
    set_field("box-sizing", "border-box")?;
    set_field("font-family", "ui-monospace, monospace")?;
    set_field("font-size", "11px")?;
    set_field("line-height", "1.35")?;
    set_field("color", "#e0e0e0")?;
    set_field("background", "#0a0a10")?;
    set_field("border", "1px solid #444")?;
    set_field("border-radius", "4px")?;
    set_field("padding", "8px")?;

    let actions = document
        .create_element("div")
        .map_err(|_| "failed to create actions")?
        .dyn_into::<web_sys::HtmlElement>()
        .map_err(|_| "actions was not an element")?;
    let actions_style = actions.style();
    let _ = actions_style.set_property("display", "flex");
    let _ = actions_style.set_property("gap", "8px");
    let _ = actions_style.set_property("justify-content", "flex-end");

    let use_btn = document
        .create_element("button")
        .map_err(|_| "failed to create use button")?
        .dyn_into::<web_sys::HtmlElement>()
        .map_err(|_| "use was not an element")?;
    use_btn.set_text_content(Some("Import"));
    use_btn
        .set_attribute("type", "button")
        .map_err(|_| "button type")?;
    style_dialog_button(&use_btn.style(), true)?;

    let close = document
        .create_element("button")
        .map_err(|_| "failed to create close button")?
        .dyn_into::<web_sys::HtmlElement>()
        .map_err(|_| "close was not an element")?;
    close.set_text_content(Some("Cancel"));
    close
        .set_attribute("type", "button")
        .map_err(|_| "button type")?;
    style_dialog_button(&close.style(), false)?;

    panel.append_child(&field).map_err(|_| "append field")?;
    actions
        .append_child(&close)
        .map_err(|_| "append cancel")?;
    actions
        .append_child(&use_btn)
        .map_err(|_| "append use")?;
    panel
        .append_child(&actions)
        .map_err(|_| "append actions")?;
    overlay
        .append_child(&panel)
        .map_err(|_| "append panel")?;
    body.append_child(&overlay)
        .map_err(|_| "append paste modal")?;

    let _ = field.focus();

    let field = std::rc::Rc::new(field);
    wire_paste_modal(document, &overlay, &close, &use_btn, field, sender)?;

    Ok(())
}

fn style_dialog_button(
    style: &web_sys::CssStyleDeclaration,
    primary: bool,
) -> Result<(), String> {
    let set = |prop: &str, value: &str| {
        style
            .set_property(prop, value)
            .map_err(|_| format!("button style {prop}"))
    };
    set("padding", "6px 14px")?;
    set("font-size", "13px")?;
    set("border-radius", "4px")?;
    set("border", "1px solid #555")?;
    if primary {
        set("background", "#2a4a8a")?;
        set("color", "#eee")?;
    } else {
        set("background", "#222")?;
        set("color", "#eee")?;
    }
    Ok(())
}

fn wire_paste_modal(
    document: &web_sys::Document,
    overlay: &web_sys::HtmlElement,
    close: &web_sys::HtmlElement,
    use_btn: &web_sys::HtmlElement,
    field: std::rc::Rc<web_sys::HtmlTextAreaElement>,
    sender: Sender<Result<String, String>>,
) -> Result<(), String> {
    let doc_for_close = document.clone();
    let on_close = Closure::wrap(Box::new(move |_event: web_sys::MouseEvent| {
        dismiss_paste_modal(&doc_for_close);
    }) as Box<dyn FnMut(_)>);
    close
        .add_event_listener_with_callback("click", on_close.as_ref().unchecked_ref())
        .map_err(|_| "cancel listener")?;
    on_close.forget();

    let doc_for_backdrop = document.clone();
    let overlay_for_backdrop = overlay.clone();
    let on_backdrop = Closure::wrap(Box::new(move |event: web_sys::MouseEvent| {
        if event.target() == Some(overlay_for_backdrop.clone().unchecked_into()) {
            dismiss_paste_modal(&doc_for_backdrop);
        }
    }) as Box<dyn FnMut(_)>);
    overlay
        .add_event_listener_with_callback("click", on_backdrop.as_ref().unchecked_ref())
        .map_err(|_| "backdrop listener")?;
    on_backdrop.forget();

    let doc_for_use = document.clone();
    let on_use = Closure::wrap(Box::new(move |_event: web_sys::MouseEvent| {
        let text = field.value();
        let _ = sender.send(Ok(text));
        dismiss_paste_modal(&doc_for_use);
    }) as Box<dyn FnMut(_)>);
    use_btn
        .add_event_listener_with_callback("click", on_use.as_ref().unchecked_ref())
        .map_err(|_| "use listener")?;
    on_use.forget();

    Ok(())
}

/// Delivers pasted share codes from the DOM paste dialog into egui state.
#[derive(Resource)]
pub struct WasmShareCodePaste {
    sender: Sender<Result<String, String>>,
    receiver: Receiver<Result<String, String>>,
}

impl Default for WasmShareCodePaste {
    fn default() -> Self {
        let (sender, receiver) = unbounded();
        Self { sender, receiver }
    }
}

impl WasmShareCodePaste {
    /// Real `<textarea>` paste dialog (works in itch.io iframes; no Clipboard API read).
    pub fn open_paste_dialog(&self, initial: &str) {
        let Some(window) = web_sys::window() else {
            return;
        };
        let Some(document) = window.document() else {
            return;
        };
        let Some(body) = document.body() else {
            return;
        };
        let _ = show_paste_modal(&document, &body, initial, self.sender.clone());
    }
}

pub fn poll_wasm_share_code_paste(
    mut ui_state: ResMut<UiState>,
    paste: ResMut<WasmShareCodePaste>,
) {
    while let Ok(result) = paste.receiver.try_recv() {
        if let Ok(text) = result {
            ui_state.share_code_input = text;
            ui_state.share_code_import_pending = true;
        }
    }
}
