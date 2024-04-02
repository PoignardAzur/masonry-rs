// NOPE DON'T COMMIT

/*
// This software is licensed under Apache License 2.0 and distributed on an
// "as-is" basis without warranties of any kind. See the LICENSE file for
// details.

//! A textbox widget.

use std::sync::Arc;
use std::time::Duration;

use smallvec::{smallvec, SmallVec};
use tracing::{trace_span, Span};
use vello::Scene;

use crate::action::Action;
use crate::kurbo::{Affine, Insets};
use crate::shell::{HotKey, KeyEvent, SysMods, TimerToken};
use crate::text::{ImeInvalidation, Selection, TextAlignment, TextComponent, TextLayout};
use crate::widget::{Portal, WidgetMut, WidgetRef};
use crate::{
    theme, ArcStr, BoxConstraints, Command, Event, EventCtx, LayoutCtx, LifeCycle, LifeCycleCtx,
    PaintCtx, Point, Rect, Size, StatusChange, Vec2, Widget, WidgetPod,
};
use druid_shell::piet::TextLayout as _;

const CURSOR_BLINK_DURATION: Duration = Duration::from_millis(500);
const MAC_OR_LINUX: bool = cfg!(any(target_os = "macos", target_os = "linux"));

// TODO - Implement formatters (TextBox where the text represents a value of some other type).

// TODO
#[allow(dead_code)]
/// When we scroll after editing or movement, we show a little extra of the document.
const SCROLL_TO_INSETS: Insets = Insets::uniform_xy(40.0, 0.0);

/// A widget that allows user text input.
pub struct TextBox {
    placeholder_text: ArcStr,
    placeholder_layout: TextLayout<ArcStr>,
    // TODO - rename inner
    // TODO - Add padding
    inner: WidgetPod<Portal<TextComponent<Arc<String>>>>,
    scroll_to_selection_after_layout: bool,
    multiline: bool,
    /// true if a click event caused us to gain focus.
    ///
    /// On macOS, if focus happens via click then we set the selection based
    /// on the click position; if focus happens automatically (e.g. on tab)
    /// then we select our entire contents.
    was_focused_from_click: bool,
    cursor_on: bool,
    cursor_timer: TimerToken,
    /// if `true` (the default), this textbox will attempt to change focus on tab.
    ///
    /// You can override this in a controller if you want to customize tab
    /// behaviour.
    pub handles_tab_notifications: bool,
    // TODO
    #[allow(dead_code)]
    text_pos: Point,
}
crate::declare_widget!(TextBoxMut, TextBox);

impl TextBox {
    /// Create a new TextBox widget.
    pub fn new(initial_text: impl Into<String>) -> Self {
        let placeholder_text = ArcStr::from("");
        let mut placeholder_layout = TextLayout::new();
        placeholder_layout.set_text_color(theme::PLACEHOLDER_COLOR);
        placeholder_layout.set_text(placeholder_text.clone());

        let text_component = TextComponent::new(Arc::new(initial_text.into()));
        let scroll = Portal::new(text_component).content_must_fill(true);
        //TODO
        //scroll.set_enabled_scrollbars(crate::scroll_component::ScrollbarsEnabled::None);
        Self {
            inner: WidgetPod::new(scroll),
            scroll_to_selection_after_layout: false,
            placeholder_text,
            placeholder_layout,
            multiline: false,
            was_focused_from_click: false,
            cursor_on: false,
            cursor_timer: TimerToken::INVALID,
            handles_tab_notifications: true,
            text_pos: Point::ZERO,
        }
    }

    /// Create a new multi-line `TextBox`.
    pub fn multiline(initial_text: impl Into<String>) -> Self {
        let mut this = TextBox::new(initial_text);
        //TODO
        //this.inner.set_enabled_scrollbars(crate::scroll_component::ScrollbarsEnabled::Both);
        //this.text_mut().borrow_mut().set_accepts_newlines(true);
        //this.inner.set_horizontal_scroll_enabled(false);
        this.multiline = true;
        this
    }

    // TODO
    #[cfg(FALSE)]
    /// If `true` (and this is a [`multiline`] text box) lines will be wrapped
    /// at the maximum layout width.
    ///
    /// If `false`, lines will not be wrapped, and horizontal scrolling will
    /// be enabled.
    ///
    /// [`multiline`]: TextBox::multiline
    pub fn with_line_wrapping(mut self, wrap_lines: bool) -> Self {
        self.inner.set_horizontal_scroll_enabled(!wrap_lines);
        self
    }
}

// TODO
#[cfg(FALSE)]
impl TextBox {
    /// Builder-style method for setting the text size.
    ///
    /// The argument can be either an `f64` or a [`Key<f64>`].
    ///
    /// [`Key<f64>`]: ../struct.Key.html
    pub fn with_text_size(mut self, size: impl Into<KeyOrValue<f64>>) -> Self {
        self.set_text_size(size);
        self
    }

    /// Builder-style method to set the [`TextAlignment`].
    ///
    /// This is only relevant when the `TextBox` is *not* [`multiline`],
    /// in which case it determines how the text is positioned inside the
    /// `TextBox` when it does not fill the available space.
    ///
    /// # Note:
    ///
    /// This does not behave exactly like [`TextAlignment`] does when used
    /// with label; in particular this does not account for reading direction.
    /// This means that `TextAlignment::Start` (the default) always means
    /// *left aligned*, and `TextAlignment::End` always means *right aligned*.
    ///
    /// This should be considered a bug, but it will not be fixed until proper
    /// BiDi support is implemented.
    ///
    /// [`TextAlignment`]: enum.TextAlignment.html
    /// [`multiline`]: #method.multiline
    pub fn with_text_alignment(mut self, alignment: TextAlignment) -> Self {
        self.set_text_alignment(alignment);
        self
    }

    /// Builder-style method for setting the font.
    pub fn with_font(mut self, font: FontDescriptor) -> Self {
        self.set_font(font);
        self
    }

    /// Builder-style method for setting the text color.
    ///
    /// The argument can be either a `Color` or a [`Key<Color>`].
    ///
    /// [`Key<Color>`]: ../struct.Key.html
    pub fn with_text_color(mut self, color: impl Into<KeyOrValue<Color>>) -> Self {
        self.set_text_color(color);
        self
    }

    /// Set the text size.
    ///
    /// The argument can be either an `f64` or a [`Key<f64>`].
    ///
    /// [`Key<f64>`]: ../struct.Key.html
    pub fn set_text_size(&mut self, size: impl Into<KeyOrValue<f64>>) {
        if !self.inner.as_ref().child().can_write() {
            tracing::warn!("set_text_size called with IME lock held.");
            return;
        }

        let size = size.into();
        self.text_mut()
            .borrow_mut()
            .layout
            .set_text_size(size.clone());
        self.placeholder_layout.set_text_size(size);
    }

    /// Set the font.
    pub fn set_font(&mut self, font: FontDescriptor) {
        if !self.inner.as_ref().child().can_write() {
            tracing::warn!("set_font called with IME lock held.");
            return;
        }
        self.text_mut().borrow_mut().layout.set_font(font.clone());
        self.placeholder_layout.set_font(font);
    }

    /// Set the [`TextAlignment`] for this `TextBox``.
    ///
    /// This is only relevant when the `TextBox` is *not* [`multiline`],
    /// in which case it determines how the text is positioned inside the
    /// `TextBox` when it does not fill the available space.
    ///
    /// # Note:
    ///
    /// This does not behave exactly like [`TextAlignment`] does when used
    /// with label; in particular this does not account for reading direction.
    /// This means that `TextAlignment::Start` (the default) always means
    /// *left aligned*, and `TextAlignment::End` always means *right aligned*.
    ///
    /// This should be considered a bug, but it will not be fixed until proper
    /// BiDi support is implemented.
    ///
    /// [`TextAlignment`]: enum.TextAlignment.html
    /// [`multiline`]: #method.multiline
    pub fn set_text_alignment(&mut self, alignment: TextAlignment) {
        if !self.inner.as_ref().child().can_write() {
            tracing::warn!("set_text_alignment called with IME lock held.");
            return;
        }
        self.text_mut().borrow_mut().set_text_alignment(alignment);
    }

    /// Set the text color.
    ///
    /// The argument can be either a `Color` or a [`Key<Color>`].
    ///
    /// If you change this property, you are responsible for calling
    /// [`request_layout`] to ensure the label is updated.
    ///
    /// [`request_layout`]: ../struct.EventCtx.html#method.request_layout
    /// [`Key<Color>`]: ../struct.Key.html
    pub fn set_text_color(&mut self, color: impl Into<KeyOrValue<Color>>) {
        if !self.inner.as_ref().child().can_write() {
            tracing::warn!("set_text_color calld with IME lock held.");
            return;
        }
        self.text_mut().borrow_mut().layout.set_text_color(color);
    }

    /// The point, relative to the origin, where this text box draws its
    /// [`TextLayout`].
    ///
    /// This is exposed in case the user wants to do additional drawing based
    /// on properties of the text.
    ///
    /// This is not valid until `layout` has been called.
    pub fn text_position(&self) -> Point {
        self.text_pos
    }
}

impl TextBox {
    /// Builder-style method to set the `TextBox`'s placeholder text.
    pub fn with_placeholder(mut self, placeholder: impl Into<ArcStr>) -> Self {
        self.set_placeholder(placeholder);
        self
    }

    // TODO
    /// Set the `TextBox`'s placeholder text.
    fn set_placeholder(&mut self, placeholder: impl Into<ArcStr>) {
        self.placeholder_text = placeholder.into();
        self.placeholder_layout
            .set_text(self.placeholder_text.clone());
    }
}

impl TextBox {
    // TODO - Return &str
    /// Return the box's current contents.
    pub fn text(&self) -> String {
        self.inner
            .as_ref()
            .child()
            .borrow()
            .layout
            .text()
            .map(|txt| txt.as_str())
            .unwrap_or("")
            .to_string()
    }

    fn text_len(&self) -> usize {
        self.inner.as_ref().child().borrow().layout.text_len()
    }

    fn reset_cursor_blink(&mut self, token: TimerToken) {
        self.cursor_on = true;
        self.cursor_timer = token;
    }

    fn should_draw_cursor(&self) -> bool {
        if cfg!(target_os = "macos") && self.inner.as_ref().child().can_read() {
            self.cursor_on && self.inner.as_ref().child().borrow().selection().is_caret()
        } else {
            self.cursor_on
        }
    }
}

impl<'a, 'b> TextBoxMut<'a, 'b> {
    pub fn inner_mut(&mut self) -> WidgetMut<'_, 'b, Portal<TextComponent<Arc<String>>>> {
        self.ctx.get_mut(&mut self.widget.inner)
    }

    pub fn set_text(&mut self, new_text: impl Into<String>) {
        self.inner_mut().child_mut().set_text(new_text.into());
    }
}

impl TextBox {
    fn rect_for_selection_end(&self) -> Rect {
        // TODO
        let child = self.inner.as_ref();
        let child = child.child();
        let text = child.borrow();
        let layout = text.layout.layout().unwrap();

        let hit = layout.hit_test_text_position(text.selection().active);
        let line = layout.line_metric(hit.line).unwrap();
        let y0 = line.y_offset;
        let y1 = y0 + line.height;
        let x = hit.point.x;

        Rect::new(x, y0, x, y1)
    }

    #[cfg(FALSE)]
    fn scroll_to_selection_end(&mut self) {
        let rect = self.rect_for_selection_end();
        let view_rect = self.inner.viewport_rect();
        let is_visible =
            view_rect.contains(rect.origin()) && view_rect.contains(Point::new(rect.x1, rect.y1));
        if !is_visible {
            self.inner.scroll_to(rect + SCROLL_TO_INSETS);
        }
    }

    /// These commands may be supplied by menus; but if they aren't, we
    /// inject them again, here.
    fn fallback_do_builtin_command(
        &mut self,
        ctx: &mut EventCtx,
        key: &KeyEvent,
    ) -> Option<Command> {
        let our_id = ctx.widget_id();
        match key {
            key if HotKey::new(SysMods::Cmd, "c").matches(key) => {
                Some(crate::command::COPY.to(our_id))
            }
            key if HotKey::new(SysMods::Cmd, "x").matches(key) => {
                Some(crate::command::CUT.to(our_id))
            }
            // we have to send paste to the window, in order to get it converted into the `Paste`
            // event
            key if HotKey::new(SysMods::Cmd, "v").matches(key) => {
                Some(crate::command::PASTE.to(ctx.window_id()))
            }
            key if HotKey::new(SysMods::Cmd, "z").matches(key) => {
                Some(crate::command::UNDO.to(our_id))
            }
            key if HotKey::new(SysMods::CmdShift, "Z").matches(key) && !cfg!(windows) => {
                Some(crate::command::REDO.to(our_id))
            }
            key if HotKey::new(SysMods::Cmd, "y").matches(key) && cfg!(windows) => {
                Some(crate::command::REDO.to(our_id))
            }
            key if HotKey::new(SysMods::Cmd, "a").matches(key) => {
                Some(crate::command::SELECT_ALL.to(our_id))
            }
            _ => None,
        }
    }
}

impl Widget for TextBox {
    fn on_event(&mut self, ctx: &mut EventCtx, event: &Event) {
        match event {
            Event::Notification(cmd) => match cmd {
                cmd if cmd.is(TextComponent::SCROLL_TO) => {
                    let after_edit = *cmd.try_get(TextComponent::SCROLL_TO).unwrap();
                    if after_edit {
                        ctx.request_layout();
                        self.scroll_to_selection_after_layout = true;
                    } else {
                        let selection_end = self.rect_for_selection_end();
                        let mut child = ctx.get_mut(&mut self.inner);
                        child.pan_viewport_to(selection_end);
                    }
                    ctx.set_handled();
                    ctx.request_paint();
                }
                cmd if cmd.is(TextComponent::TAB) && self.handles_tab_notifications => {
                    ctx.focus_next();
                    ctx.request_paint();
                    ctx.set_handled();
                }
                cmd if cmd.is(TextComponent::BACKTAB) && self.handles_tab_notifications => {
                    ctx.focus_prev();
                    ctx.request_paint();
                    ctx.set_handled();
                }
                cmd if cmd.is(TextComponent::CANCEL) => {
                    ctx.resign_focus();
                    ctx.request_paint();
                    ctx.set_handled();
                }
                cmd if cmd.is(TextComponent::TEXT_CHANGED) => {
                    // TODO - remove clones
                    let text = cmd.try_get(TextComponent::TEXT_CHANGED).unwrap();
                    ctx.submit_action(Action::TextChanged(text.clone()));
                    ctx.set_handled();
                }
                cmd if cmd.is(TextComponent::RETURN) => {
                    // TODO - remove clones
                    let text = cmd.try_get(TextComponent::RETURN).unwrap();
                    ctx.submit_action(Action::TextEntered(text.clone()));
                    ctx.set_handled();
                }
                _ => (),
            },
            Event::KeyDown(key) if !self.inner.as_ref().child().is_composing() => {
                if let Some(cmd) = self.fallback_do_builtin_command(ctx, key) {
                    ctx.submit_command(cmd);
                    ctx.set_handled();
                }
            }
            Event::MouseDown(mouse) if self.inner.as_ref().child().can_write() => {
                if !ctx.is_disabled() {
                    if !mouse.focus {
                        ctx.request_focus();
                        self.was_focused_from_click = true;
                        self.reset_cursor_blink(ctx.request_timer(CURSOR_BLINK_DURATION));
                    } else {
                        ctx.set_handled();
                    }
                }
            }
            Event::Timer(id) => {
                if !ctx.is_disabled() {
                    if *id == self.cursor_timer && ctx.has_focus() {
                        self.cursor_on = !self.cursor_on;
                        ctx.request_paint();
                        self.cursor_timer = ctx.request_timer(CURSOR_BLINK_DURATION);
                    }
                } else if self.cursor_on {
                    self.cursor_on = false;
                    ctx.request_paint();
                }
            }
            Event::ImeStateChange => {
                self.reset_cursor_blink(ctx.request_timer(CURSOR_BLINK_DURATION));
                // TODO - external_text_change.is_some()
            }
            Event::Command(ref cmd)
                if !self.inner.as_ref().child().is_composing()
                    && ctx.is_focused()
                    && cmd.is(crate::command::COPY) =>
            {
                self.inner.as_ref().child().borrow().set_clipboard();
                ctx.set_handled();
            }
            Event::Command(cmd)
                if !self.inner.as_ref().child().is_composing()
                    && ctx.is_focused()
                    && cmd.is(crate::command::CUT) =>
            {
                // TODO
                #[cfg(FALSE)]
                if self.text().borrow().set_clipboard() {
                    let inval = self.text_mut().borrow_mut().insert_text(data, "");
                    ctx.invalidate_text_input(inval);
                }
                ctx.set_handled();
            }
            Event::Command(cmd)
                if !self.inner.as_ref().child().is_composing()
                    && ctx.is_focused()
                    && cmd.is(crate::command::SELECT_ALL) =>
            {
                // TODO
                #[cfg(FALSE)]
                if let Some(inval) = self
                    .text_mut()
                    .borrow_mut()
                    .set_selection(Selection::new(0, data.as_str().len()))
                {
                    ctx.invalidate_text_input(inval);
                }
                ctx.set_handled();
            }
            Event::Paste(ref item) if self.inner.as_ref().child().can_write() => {
                if let Some(string) = item.get_string() {
                    let _text = if self.multiline {
                        &string
                    } else {
                        string.lines().next().unwrap_or("")
                    };
                    // TODO
                    #[cfg(FALSE)]
                    if !text.is_empty() {
                        let inval = self.text_mut().borrow_mut().insert_text(data, text);
                        ctx.invalidate_text_input(inval);
                    }
                }
            }
            _ => (),
        }
        self.inner.on_event(ctx, event)
    }

    fn on_status_change(&mut self, ctx: &mut LifeCycleCtx, event: &StatusChange) {
        match event {
            StatusChange::FocusChanged(true) => {
                // TODO
                #[cfg(FALSE)]
                if self.text().can_write() && !self.multiline && !self.was_focused_from_click {
                    let selection = Selection::new(0, data.len());
                    let _ = self.text_mut().borrow_mut().set_selection(selection);
                    ctx.invalidate_text_input(ImeInvalidation::SelectionChanged);
                }

                {
                    let mut child = ctx.get_mut(&mut self.inner);
                    child.child_mut().set_focused(true);
                }
                self.reset_cursor_blink(ctx.request_timer(CURSOR_BLINK_DURATION));
                self.was_focused_from_click = false;
                ctx.request_paint();
            }
            StatusChange::FocusChanged(false) => {
                if self.inner.as_ref().child().can_write() && MAC_OR_LINUX && !self.multiline {
                    let selection = self.inner.as_ref().child().borrow().selection();
                    let selection = Selection::new(selection.active, selection.active);
                    let _ = self
                        .inner
                        .as_ref()
                        .child()
                        .borrow_mut()
                        .set_selection(selection);
                    ctx.invalidate_text_input(ImeInvalidation::SelectionChanged);
                }

                {
                    let mut child = ctx.get_mut(&mut self.inner);
                    child.child_mut().set_focused(false);
                    if !self.multiline {
                        // TODO - remove?
                        child.pan_viewport_to(Rect::ZERO);
                    }
                }

                self.cursor_timer = TimerToken::INVALID;
                self.was_focused_from_click = false;
                ctx.request_paint();
            }
            _ => (),
        }
    }

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, event: &LifeCycle) {
        match event {
            LifeCycle::WidgetAdded => {
                ctx.register_text_input(self.inner.as_ref().child().input_handler());
            }
            LifeCycle::BuildFocusChain => {
                //TODO: make this a configurable option? maybe?
                ctx.register_for_focus();
            }
            _ => (),
        }
        self.inner.lifecycle(ctx, event);
    }

    fn layout(&mut self, ctx: &mut LayoutCtx, bc: &BoxConstraints) -> Size {
        if !self.inner.as_ref().child().can_write() {
            tracing::warn!("Widget::layout called with outstanding IME lock.");
        }
        let min_width = theme::WIDE_WIDGET_WIDTH;
        let textbox_insets = theme::TEXTBOX_INSETS;

        self.placeholder_layout.rebuild_if_needed(ctx.text());
        let min_size = bc.constrain((min_width, 0.0));
        let child_bc = BoxConstraints::new(min_size, bc.max());

        let mut size = self.inner.layout(ctx, &child_bc);
        ctx.place_child(&mut self.inner, Point::ORIGIN);

        let text_metrics = if !self.inner.as_ref().child().can_read() || self.text_len() == 0 {
            self.placeholder_layout.layout_metrics()
        } else {
            self.inner.as_ref().child().borrow().layout.layout_metrics()
        };

        let _layout_baseline = text_metrics.size.height - text_metrics.first_baseline;
        let baseline_off = 0.0;
        // = layout_baseline - (self.inner.child_size().height - self.inner.viewport_rect().height()) + textbox_insets.y1;
        ctx.set_baseline_offset(baseline_off);
        if self.scroll_to_selection_after_layout {
            // TODO
            //self.scroll_to_selection_end();
            self.scroll_to_selection_after_layout = false;
        }

        size.width += textbox_insets.x0 + textbox_insets.x1;
        size.height += textbox_insets.y0 + textbox_insets.y1;
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, scene: &mut Scene) {
        if !self.inner.as_ref().child().can_read() {
            tracing::warn!("Widget::paint called with outstanding IME lock, skipping");
            return;
        }
        let size = ctx.size();
        let background_color = theme::BACKGROUND_LIGHT;
        let cursor_color = theme::CURSOR_COLOR;
        let border_width = theme::TEXTBOX_BORDER_WIDTH;
        let textbox_insets = theme::TEXTBOX_INSETS;

        let is_focused = ctx.is_focused();

        let border_color = if is_focused {
            theme::PRIMARY_LIGHT
        } else {
            theme::BORDER_DARK
        };

        // Paint the background
        let clip_rect = size
            .to_rect()
            .inset(-border_width / 2.0)
            .to_rounded_rect(theme::TEXTBOX_BORDER_RADIUS);

        ctx.fill(clip_rect, &background_color);

        if self.text_len() != 0 {
            let padding_offset = Vec2::new(textbox_insets.x0, textbox_insets.y0);
            ctx.with_save(|ctx| {
                ctx.transform(Affine::translate(padding_offset));
                self.inner.paint(ctx, scene);
            })
        } else {
            ctx.skip_child(&mut self.inner);

            let text_width = self.placeholder_layout.layout_metrics().size.width;
            let extra_width = (size.width - text_width - textbox_insets.x_value()).max(0.);
            let alignment = self.inner.as_ref().child().borrow().text_alignment();
            // alignment is only used for single-line text boxes
            let x_offset = if self.multiline {
                0.0
            } else {
                x_offset_for_extra_width(alignment, extra_width)
            };

            // clip when we draw the placeholder, since it isn't in a clipbox
            ctx.with_save(|ctx| {
                ctx.clip(clip_rect);
                self.placeholder_layout
                    .draw(ctx, (textbox_insets.x0 + x_offset, textbox_insets.y0));
            })
        }

        // Paint the cursor if focused and there's no selection
        if is_focused && self.should_draw_cursor() {
            // if there's no data, we always draw the cursor based on
            // our alignment.
            let cursor_pos = self.inner.as_ref().child().borrow().selection().active;
            let cursor_line = self
                .inner
                .as_ref()
                .child()
                .borrow()
                .cursor_line_for_text_position(cursor_pos);

            let padding_offset = Vec2::new(textbox_insets.x0, textbox_insets.y0);

            let mut cursor = if self.text_len() == 0 {
                cursor_line + padding_offset
            } else {
                cursor_line + padding_offset - self.inner.as_ref().get_viewport_pos().to_vec2()
            };

            // Snap the cursor to the pixel grid so it stays sharp.
            cursor.p0.x = cursor.p0.x.trunc() + 0.5;
            cursor.p1.x = cursor.p0.x;

            ctx.with_save(|ctx| {
                ctx.clip(clip_rect);
                ctx.stroke(cursor, &cursor_color, 1.);
            })
        }

        // Paint the border
        ctx.stroke(clip_rect, &border_color, border_width);
    }

    fn children(&self) -> SmallVec<[WidgetRef<'_, dyn Widget>; 16]> {
        smallvec![self.inner.as_dyn()]
    }

    fn make_trace_span(&self) -> Span {
        trace_span!("TextBox")
    }
}

fn x_offset_for_extra_width(alignment: TextAlignment, extra_width: f64) -> f64 {
    match alignment {
        TextAlignment::Start | TextAlignment::Justified => 0.0,
        TextAlignment::End => extra_width,
        TextAlignment::Center => extra_width / 2.0,
    }
}

#[cfg(test)]
mod tests {
    use insta::assert_debug_snapshot;

    use super::*;
    use crate::action::Action;
    use crate::assert_render_snapshot;
    use crate::testing::{widget_ids, TestHarness, TestWidgetExt as _};

    #[test]
    fn simple_textbox() {
        let [textbox_id] = widget_ids();
        let textbox = TextBox::new("Hello").with_id(textbox_id);

        let mut harness = TestHarness::create(textbox);

        assert_debug_snapshot!(harness.root_widget());
        assert_render_snapshot!(harness, "hello");

        assert_eq!(harness.pop_action(), None);

        harness.mouse_click_on(textbox_id);
        assert_eq!(harness.focused_widget().unwrap().id(), textbox_id);
        assert_eq!(harness.pop_action(), None);

        harness.keyboard_type_chars("abc");
        assert_eq!(
            harness.pop_action(),
            Some((Action::TextChanged("a".to_string()), textbox_id))
        );
        assert_eq!(
            harness.pop_action(),
            Some((Action::TextChanged("ab".to_string()), textbox_id))
        );
        assert_eq!(
            harness.pop_action(),
            Some((Action::TextChanged("abc".to_string()), textbox_id))
        );

        dbg!(harness.get_widget(textbox_id));
        assert_eq!(
            harness
                .get_widget(textbox_id)
                .downcast::<TextBox>()
                .unwrap()
                .text(),
            "abc"
        );
    }

    #[test]
    fn simple_textbox_placeholder() {
        let textbox = TextBox::new("").with_placeholder("placeholder text");

        let mut harness = TestHarness::create(textbox);

        assert_debug_snapshot!(harness.root_widget());
        assert_render_snapshot!(harness, "placeholder");
    }

    // TODO - styled textbox

    #[test]
    fn edit_textbox() {
        // TODO - do styles
        let image_1 = {
            let textbox = TextBox::new("The quick brown fox jumps over the lazy dog");

            let mut harness = TestHarness::create_with_size(textbox, Size::new(50.0, 50.0));

            harness.render()
        };

        let image_2 = {
            let textbox = TextBox::new("Hello world");

            let mut harness = TestHarness::create_with_size(textbox, Size::new(50.0, 50.0));

            harness.edit_root_widget(|mut textbox| {
                let mut textbox = textbox.downcast::<TextBox>().unwrap();
                textbox.set_text("The quick brown fox jumps over the lazy dog");
            });

            harness.render()
        };

        // We don't use assert_eq because we don't want rich assert
        assert!(image_1 == image_2);
    }
}
*/
