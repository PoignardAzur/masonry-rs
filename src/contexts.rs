// This software is licensed under Apache License 2.0 and distributed on an
// "as-is" basis without warranties of any kind. See the LICENSE file for
// details.

//! The context types that are passed into various widget methods.

use std::any::Any;
use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use druid_shell::piet::PietText;
use druid_shell::text::Event as ImeInvalidation;
use druid_shell::{Cursor, TimerToken, WindowHandle};
use tracing::{trace, warn};

use crate::action::{Action, ActionQueue};
use crate::command::{Command, CommandQueue, Notification, SingleUse};
use crate::debug_logger::DebugLogger;
use crate::ext_event::ExtEventSink;
use crate::platform::WindowDescription;
use crate::promise::PromiseToken;
use crate::testing::MockTimerQueue;
use crate::text_helpers::TextFieldRegistration;
use crate::widget::{CursorChange, FocusChange, StoreInWidgetMut, WidgetMut, WidgetState};
use crate::{Insets, Point, Rect, Size, Target, Vec2, Widget, WidgetId, WidgetPod, WindowId};

/// A macro for implementing methods on multiple contexts.
///
/// There are a lot of methods defined on multiple contexts; this lets us only
/// have to write them out once.
macro_rules! impl_context_method {
    ($ty:ty,  { $($method:item)+ } ) => {
        impl $ty { $($method)+ }
    };
    ( $ty:ty, $($more:ty),+, { $($method:item)+ } ) => {
        impl_context_method!($ty, { $($method)+ });
        impl_context_method!($($more),+, { $($method)+ });
    };
}

// TODO - remove second lifetime, only keep queues and Rc
// TODO - rename lifetimes
/// Static state that is shared between most contexts.
pub(crate) struct GlobalPassCtx<'a> {
    pub(crate) ext_event_sink: ExtEventSink,
    pub(crate) debug_logger: &'a mut DebugLogger,
    pub(crate) command_queue: &'a mut CommandQueue,
    pub(crate) action_queue: &'a mut ActionQueue,
    // TODO - merge queues
    // Associate timers with widgets that requested them.
    pub(crate) timers: &'a mut HashMap<TimerToken, WidgetId>,
    // Used in Harness for unit tests - see `src/testing/mock_timer_queue.rs`
    pub(crate) mock_timer_queue: Option<&'a mut MockTimerQueue>,
    pub(crate) window_id: WindowId,
    pub(crate) window: &'a WindowHandle,
    pub(crate) text: PietText,
    /// The id of the widget that currently has focus.
    pub(crate) focus_widget: Option<WidgetId>,
}

/// A context provided to implementors of [`StoreInWidgetMut`].
///
/// When you declare a mutable reference type for your widget, methods of this type
/// will have access to a `WidgetCtx`. If that method mutates the widget in a way that
/// requires a later pass (for instance, if your widget has a `set_color` method),
/// you will need to signal that change in the pass (eg `requrest_paint`).
///
// TODO add tutorial - See issue #5
pub struct WidgetCtx<'a, 'b> {
    pub(crate) global_state: &'a mut GlobalPassCtx<'b>,
    pub(crate) widget_state: &'a mut WidgetState,
}

/// A context provided to event handling methods of widgets.
///
/// Widgets should call [`request_paint`](Self::request_paint) whenever an event causes a change
/// in the widget's appearance, to schedule a repaint.
pub struct EventCtx<'a, 'b> {
    pub(crate) global_state: &'a mut GlobalPassCtx<'b>,
    pub(crate) widget_state: &'a mut WidgetState,
    pub(crate) notifications: &'a mut VecDeque<Notification>,
    pub(crate) is_handled: bool,
    pub(crate) is_root: bool,
    pub(crate) request_pan_to_child: Option<Rect>,
}

/// A context provided to the [`lifecycle`] method on widgets.
///
/// [`lifecycle`]: trait.Widget.html#tymethod.lifecycle
pub struct LifeCycleCtx<'a, 'b> {
    pub(crate) global_state: &'a mut GlobalPassCtx<'b>,
    pub(crate) widget_state: &'a mut WidgetState,
}

/// A context provided to layout handling methods of widgets.
///
/// As of now, the main service provided is access to a factory for
/// creating text layout objects, which are likely to be useful
/// during widget layout.
pub struct LayoutCtx<'a, 'b> {
    pub(crate) global_state: &'a mut GlobalPassCtx<'b>,
    pub(crate) widget_state: &'a mut WidgetState,
    pub(crate) mouse_pos: Option<Point>,
}

/// A context passed to paint methods of widgets.
pub struct PaintCtx<'a, 'b> {
    pub(crate) global_state: &'a mut GlobalPassCtx<'b>,
    pub(crate) widget_state: &'a WidgetState,
    /// The approximate depth in the tree at the time of painting.
    pub(crate) depth: u32,
    pub(crate) debug_paint: bool,
    pub(crate) debug_widget: bool,
}

impl_context_method!(
    WidgetCtx<'_, '_>,
    EventCtx<'_, '_>,
    LifeCycleCtx<'_, '_>,
    PaintCtx<'_, '_>,
    LayoutCtx<'_, '_>,
    {
        /// get the `WidgetId` of the current widget.
        pub fn widget_id(&self) -> WidgetId {
            self.widget_state.id
        }

        /// Returns a reference to the current `WindowHandle`.
        pub fn window(&self) -> &WindowHandle {
            self.global_state.window
        }

        /// Get the `WindowId` of the current window.
        pub fn window_id(&self) -> WindowId {
            self.global_state.window_id
        }

        /// Get an object which can create text layouts.
        pub fn text(&mut self) -> &mut PietText {
            &mut self.global_state.text
        }

        /// Skip iterating over the given child.
        ///
        /// Normally, container widgets are supposed to iterate over each of their
        /// child widgets in their methods. By default, the framework treats not
        /// doing so as a mistake, and panics if debug assertions are on.
        ///
        /// This tells the framework that a child was deliberately skipped.
        // TODO - see event flow tutorial - See issue #5
        pub fn skip_child(&self, child: &mut WidgetPod<impl Widget>) {
            child.mark_as_visited();
        }
    }
);

// methods on everyone but layoutctx
impl_context_method!(
    WidgetCtx<'_, '_>,
    EventCtx<'_, '_>,
    LifeCycleCtx<'_, '_>,
    PaintCtx<'_, '_>,
    {
        /// The layout size.
        ///
        /// This is the layout size as ultimately determined by the parent
        /// container, on the previous layout pass.
        ///
        /// Generally it will be the same as the size returned by the child widget's
        /// [`layout`] method.
        ///
        /// [`layout`]: trait.Widget.html#tymethod.layout
        pub fn size(&self) -> Size {
            self.widget_state.size()
        }

        /// The origin of the widget in window coordinates, relative to the top left corner of the
        /// content area.
        pub fn window_origin(&self) -> Point {
            self.widget_state.window_origin()
        }

        /// Convert a point from the widget's coordinate space to the window's.
        ///
        /// The returned point is relative to the content area; it excludes window chrome.
        pub fn to_window(&self, widget_point: Point) -> Point {
            self.window_origin() + widget_point.to_vec2()
        }

        /// Convert a point from the widget's coordinate space to the screen's.
        /// See the [`Screen`] module
        ///
        /// [`Screen`]: druid_shell::Screen
        pub fn to_screen(&self, widget_point: Point) -> Point {
            let insets = self.window().content_insets();
            let window_pos = self.window().get_position();
            let window_pos = Point::new(window_pos.x, window_pos.y);
            let content_origin = window_pos + Vec2::new(insets.x0, insets.y0);
            content_origin + self.to_window(widget_point).to_vec2()
        }

        /// The "hot" (aka hover) status of a widget.
        ///
        /// A widget is "hot" when the mouse is hovered over it. Widgets will
        /// often change their appearance as a visual indication that they
        /// will respond to mouse interaction.
        ///
        /// The hot status is computed from the widget's layout rect. In a
        /// container hierarchy, all widgets with layout rects containing the
        /// mouse position have hot status.
        ///
        /// Discussion: there is currently some confusion about whether a
        /// widget can be considered hot when some other widget is active (for
        /// example, when clicking to one widget and dragging to the next).
        /// The documentation should clearly state the resolution.
        pub fn is_hot(&self) -> bool {
            self.widget_state.is_hot
        }

        /// The active status of a widget.
        ///
        /// Active status generally corresponds to a mouse button down. Widgets
        /// with behavior similar to a button will call [`set_active`](EventCtx::set_active) on mouse
        /// down and then up.
        ///
        /// When a widget is active, it gets mouse events even when the mouse
        /// is dragged away.
        pub fn is_active(&self) -> bool {
            self.widget_state.is_active
        }

        /// The focus status of a widget.
        ///
        /// Returns `true` if this specific widget is focused.
        /// To check if any descendants are focused use [`has_focus`].
        ///
        /// Focus means that the widget receives keyboard events.
        ///
        /// A widget can request focus using the [`request_focus`] method.
        /// It's also possible to register for automatic focus via [`register_for_focus`].
        ///
        /// If a widget gains or loses focus it will get a [`LifeCycle::FocusChanged`] event.
        ///
        /// Only one widget at a time is focused. However due to the way events are routed,
        /// all ancestors of that widget will also receive keyboard events.
        ///
        /// [`request_focus`]: struct.EventCtx.html#method.request_focus
        /// [`register_for_focus`]: struct.LifeCycleCtx.html#method.register_for_focus
        /// [`LifeCycle::FocusChanged`]: enum.LifeCycle.html#variant.FocusChanged
        /// [`has_focus`]: #method.has_focus
        pub fn is_focused(&self) -> bool {
            self.global_state.focus_widget == Some(self.widget_id())
        }

        /// The (tree) focus status of a widget.
        ///
        /// Returns `true` if either this specific widget or any one of its descendants is focused.
        /// To check if only this specific widget is focused use [`is_focused`](Self::is_focused).
        pub fn has_focus(&self) -> bool {
            self.widget_state.has_focus
        }

        /// The disabled state of a widget.
        ///
        /// Returns `true` if this widget or any of its ancestors is explicitly disabled.
        /// To make this widget explicitly disabled use [`set_disabled`].
        ///
        /// Disabled means that this widget should not change the state of the application. What
        /// that means is not entirely clear but in any it should not change its data. Therefore
        /// others can use this as a safety mechanism to prevent the application from entering an
        /// illegal state.
        /// For an example the decrease button of a counter of type `usize` should be disabled if the
        /// value is `0`.
        ///
        /// [`set_disabled`]: EventCtx::set_disabled
        pub fn is_disabled(&self) -> bool {
            self.widget_state.is_disabled()
        }

        /// Check is widget is stashed.
        ///
        /// **Note:** Stashed widgets are a WIP feature
        // FIXME - take stashed parents into account
        pub fn is_stashed(&self) -> bool {
            self.widget_state.is_stashed
        }
    }
);

impl_context_method!(EventCtx<'_, '_>, {
    /// Set the cursor icon.
    ///
    /// This setting will be retained until [`clear_cursor`] is called, but it will only take
    /// effect when this widget is either [`hot`] or [`active`]. If a child widget also sets a
    /// cursor, the child widget's cursor will take precedence. (If that isn't what you want, use
    /// [`override_cursor`] instead.)
    ///
    /// [`clear_cursor`]: EventCtx::clear_cursor
    /// [`override_cursor`]: EventCtx::override_cursor
    /// [`hot`]: EventCtx::is_hot
    /// [`active`]: EventCtx::is_active
    pub fn set_cursor(&mut self, cursor: &Cursor) {
        trace!("set_cursor {:?}", cursor);
        self.widget_state.cursor_change = CursorChange::Set(cursor.clone());
    }

    /// Override the cursor icon.
    ///
    /// This setting will be retained until [`clear_cursor`] is called, but it will only take
    /// effect when this widget is either [`hot`] or [`active`]. This will override the cursor
    /// preferences of a child widget. (If that isn't what you want, use [`set_cursor`] instead.)
    ///
    /// [`clear_cursor`]: EventCtx::clear_cursor
    /// [`set_cursor`]: EventCtx::override_cursor
    /// [`hot`]: EventCtx::is_hot
    /// [`active`]: EventCtx::is_active
    pub fn override_cursor(&mut self, cursor: &Cursor) {
        trace!("override_cursor {:?}", cursor);
        self.widget_state.cursor_change = CursorChange::Override(cursor.clone());
    }

    /// Clear the cursor icon.
    ///
    /// This undoes the effect of [`set_cursor`] and [`override_cursor`].
    ///
    /// [`override_cursor`]: EventCtx::override_cursor
    /// [`set_cursor`]: EventCtx::set_cursor
    pub fn clear_cursor(&mut self) {
        trace!("clear_cursor");
        self.widget_state.cursor_change = CursorChange::Default;
    }
});

impl<'a, 'b> WidgetCtx<'a, 'b> {
    // FIXME - Assert that child's parent is self
    /// Return a [`WidgetMut`] to a child widget.
    pub fn get_mut<'c, Child: Widget + StoreInWidgetMut>(
        &'c mut self,
        child: &'c mut WidgetPod<Child>,
    ) -> WidgetMut<'c, 'b, Child> {
        let child_ctx = WidgetCtx {
            global_state: self.global_state,
            widget_state: &mut child.state,
        };
        WidgetMut {
            parent_widget_state: self.widget_state,
            inner: Child::from_widget_and_ctx(&mut child.inner, child_ctx),
        }
    }
}

impl<'a, 'b> EventCtx<'a, 'b> {
    /// Return a [`WidgetMut`] to a child widget.
    // FIXME - Assert that child's parent is self
    pub fn get_mut<'c, Child: Widget + StoreInWidgetMut>(
        &'c mut self,
        child: &'c mut WidgetPod<Child>,
    ) -> WidgetMut<'c, 'b, Child> {
        let child_ctx = WidgetCtx {
            global_state: self.global_state,
            widget_state: &mut child.state,
        };
        WidgetMut {
            parent_widget_state: self.widget_state,
            inner: Child::from_widget_and_ctx(&mut child.inner, child_ctx),
        }
    }
}

impl<'a, 'b> LifeCycleCtx<'a, 'b> {
    /// Return a [`WidgetMut`] to a child widget.
    // FIXME - Assert that child's parent is self
    pub fn get_mut<'c, Child: Widget + StoreInWidgetMut>(
        &'c mut self,
        child: &'c mut WidgetPod<Child>,
    ) -> WidgetMut<'c, 'b, Child> {
        let child_ctx = WidgetCtx {
            global_state: self.global_state,
            widget_state: &mut child.state,
        };
        WidgetMut {
            parent_widget_state: self.widget_state,
            inner: Child::from_widget_and_ctx(&mut child.inner, child_ctx),
        }
    }
}

// methods on event and lifecycle
impl_context_method!(WidgetCtx<'_, '_>, EventCtx<'_, '_>, LifeCycleCtx<'_, '_>, {
    /// Request a [`paint`] pass.
    /// [`paint`]: trait.Widget.html#tymethod.paint
    pub fn request_paint(&mut self) {
        trace!("request_paint");
    }

    /// Request a layout pass.
    ///
    /// A Widget's [`layout`] method is always called when the widget tree
    /// changes, or the window is resized.
    ///
    /// If your widget would like to have layout called at any other time,
    /// (such as if it would like to change the layout of children in
    /// response to some event) it must call this method.
    ///
    /// [`layout`]: trait.Widget.html#tymethod.layout
    pub fn request_layout(&mut self) {
        trace!("request_layout");
        self.widget_state.needs_layout = true;
    }

    /// Request an animation frame.
    pub fn request_anim_frame(&mut self) {
        trace!("request_anim_frame");
        self.widget_state.request_anim = true;
    }

    /// Indicate that your children have changed.
    ///
    /// Widgets must call this method after adding a new child or removing a child.
    pub fn children_changed(&mut self) {
        trace!("children_changed");
        self.widget_state.children_changed = true;
        self.widget_state.update_focus_chain = true;
        self.request_layout();
    }

    /// Set the disabled state for this widget.
    ///
    /// Setting this to `false` does not mean a widget is not still disabled; for instance it may
    /// still be disabled by an ancestor. See [`is_disabled`] for more information.
    ///
    /// Calling this method during [`LifeCycle::DisabledChanged`] has no effect.
    ///
    /// [`LifeCycle::DisabledChanged`]: struct.LifeCycle.html#variant.DisabledChanged
    /// [`is_disabled`]: EventCtx::is_disabled
    pub fn set_disabled(&mut self, disabled: bool) {
        // widget_state.children_disabled_changed is not set because we want to be able to delete
        // changes that happened during DisabledChanged.
        self.widget_state.is_explicitly_disabled_new = disabled;
    }

    /// Mark child widget as stashed.
    ///
    /// **Note:** Stashed widgets are a WIP feature
    pub fn set_stashed(&mut self, child: &mut WidgetPod<impl Widget>, stashed: bool) {
        child.state.is_stashed = stashed;
        self.children_changed();
    }

    /// Indicate that text input state has changed.
    ///
    /// A widget that accepts text input should call this anytime input state
    /// (such as the text or the selection) changes as a result of a non text-input
    /// event.
    pub fn invalidate_text_input(&mut self, event: ImeInvalidation) {
        let payload = crate::command::ImeInvalidation {
            widget: self.widget_id(),
            event,
        };
        let cmd = crate::command::INVALIDATE_IME
            .with(payload)
            .to(Target::Window(self.window_id()));
        self.submit_command(cmd);
    }
});

// methods on everyone but paintctx
impl_context_method!(
    WidgetCtx<'_, '_>,
    EventCtx<'_, '_>,
    LifeCycleCtx<'_, '_>,
    LayoutCtx<'_, '_>,
    {
        /// Submit a [`Command`] to be run after this event is handled.
        ///
        /// Commands are run in the order they are submitted; all commands
        /// submitted during the handling of an event are executed before
        /// the [`update`] method is called; events submitted during [`update`]
        /// are handled after painting.
        ///
        /// [`Target::Auto`] commands will be sent to the window containing the widget.
        ///
        /// [`Command`]: struct.Command.html
        /// [`update`]: trait.Widget.html#tymethod.update
        pub fn submit_command(&mut self, cmd: impl Into<Command>) {
            trace!("submit_command");
            self.global_state.submit_command(cmd.into())
        }

        /// Submit an [`Action`].
        ///
        /// Note: Actions are still a WIP feature.
        pub fn submit_action(&mut self, action: Action) {
            trace!("submit_command");
            self.global_state
                .submit_action(action, self.widget_state.id)
        }

        /// Run the provided function in the background.
        ///
        /// The function takes an [`ExtEventSink`] which it can use to send
        /// [`Command`]s back to the main thread.
        pub fn run_in_background(
            &mut self,
            background_task: impl FnOnce(ExtEventSink) + Send + 'static,
        ) {
            use std::thread;

            let ext_event_sink = self.global_state.ext_event_sink.clone();
            thread::spawn(move || {
                background_task(ext_event_sink);
            });
        }

        /// Run the provided function in the background, and send its result once it's done.
        ///
        /// The function takes an [`ExtEventSink`] which it can use to send
        /// [`Command`]s back to the main thread.
        ///
        /// Once the function returns, an [`Event::PromiseResult`](crate::Event::PromiseResult)
        /// is emitted with the return value.
        pub fn compute_in_background<T: Any + Send>(
            &mut self,
            background_task: impl FnOnce(ExtEventSink) -> T + Send + 'static,
        ) -> PromiseToken<T> {
            let token = PromiseToken::<T>::new();

            use std::thread;

            let ext_event_sink = self.global_state.ext_event_sink.clone();
            let widget_id = self.widget_state.id;
            let window_id = self.global_state.window_id;
            thread::spawn(move || {
                let result = background_task(ext_event_sink.clone());
                // TODO unwrap_or
                let _ =
                    ext_event_sink.resolve_promise(token.make_result(result), widget_id, window_id);
            });

            token
        }

        /// Request a timer event.
        ///
        /// The return value is a token, which can be used to associate the
        /// request with the event.
        pub fn request_timer(&mut self, deadline: Duration) -> TimerToken {
            trace!("request_timer deadline={:?}", deadline);
            self.global_state
                .request_timer(deadline, self.widget_state.id)
        }
    }
);

impl EventCtx<'_, '_> {
    /// Submit a [`Notification`].
    ///
    /// The provided argument can be a [`Selector`] or a [`Command`]; this lets
    /// us work with the existing API for addding a payload to a [`Selector`].
    ///
    /// If the argument is a `Command`, the command's target will be ignored.
    ///
    /// # Examples
    ///
    /// ```
    /// # use masonry::{Event, EventCtx, Selector};
    /// const IMPORTANT_EVENT: Selector<String> = Selector::new("masonry-example.important-event");
    ///
    /// fn check_event(ctx: &mut EventCtx, event: &Event) {
    ///     if is_this_the_event_we_were_looking_for(event) {
    ///         ctx.submit_notification(IMPORTANT_EVENT.with("That's the one".to_string()))
    ///     }
    /// }
    ///
    /// # fn is_this_the_event_we_were_looking_for(event: &Event) -> bool { true }
    /// ```
    ///
    /// [`Selector`]: crate::Selector
    pub fn submit_notification(&mut self, note: impl Into<Command>) {
        trace!("submit_notification");
        let note = note.into().into_notification(self.widget_state.id);
        self.notifications.push_back(note);
    }

    /// Create a new window.
    pub fn new_window(&mut self, desc: WindowDescription) {
        trace!("new_window");
        self.submit_command(
            crate::command::NEW_WINDOW
                .with(SingleUse::new(Box::new(desc)))
                .to(Target::Global),
        );
    }

    /// Send a signal to parent widgets to scroll this widget into view.
    pub fn request_pan_to_this(&mut self) {
        self.request_pan_to_child = Some(self.widget_state.layout_rect());
    }

    /// Set the "active" state of the widget.
    ///
    /// See [`EventCtx::is_active`](Self::is_active).
    pub fn set_active(&mut self, active: bool) {
        trace!("set_active({})", active);
        self.widget_state.is_active = active;
        // TODO: plumb mouse grab through to platform (through druid-shell)
    }

    /// Set the event as "handled", which stops its propagation to other
    /// widgets.
    pub fn set_handled(&mut self) {
        trace!("set_handled");
        self.is_handled = true;
    }

    /// Determine whether the event has been handled by some other widget.
    pub fn is_handled(&self) -> bool {
        self.is_handled
    }

    /// Request keyboard focus.
    ///
    /// Because only one widget can be focused at a time, multiple focus requests
    /// from different widgets during a single event cycle means that the last
    /// widget that requests focus will override the previous requests.
    ///
    /// See [`is_focused`](Self::is_focused) for more information about focus.
    pub fn request_focus(&mut self) {
        trace!("request_focus");
        // We need to send the request even if we're currently focused,
        // because we may have a sibling widget that already requested focus
        // and we have no way of knowing that yet. We need to override that
        // to deliver on the "last focus request wins" promise.
        let id = self.widget_id();
        self.widget_state.request_focus = Some(FocusChange::Focus(id));
    }

    /// Transfer focus to the widget with the given `WidgetId`.
    ///
    /// See [`is_focused`](Self::is_focused) for more information about focus.
    pub fn set_focus(&mut self, target: WidgetId) {
        trace!("set_focus target={:?}", target);
        self.widget_state.request_focus = Some(FocusChange::Focus(target));
    }

    /// Transfer focus to the next focusable widget.
    ///
    /// This should only be called by a widget that currently has focus.
    ///
    /// See [`is_focused`](Self::is_focused) for more information about focus.
    pub fn focus_next(&mut self) {
        trace!("focus_next");
        if self.has_focus() {
            self.widget_state.request_focus = Some(FocusChange::Next);
        } else {
            warn!(
                "focus_next can only be called by the currently \
                            focused widget or one of its ancestors."
            );
        }
    }

    /// Transfer focus to the previous focusable widget.
    ///
    /// This should only be called by a widget that currently has focus.
    ///
    /// See [`is_focused`](Self::is_focused) for more information about focus.
    pub fn focus_prev(&mut self) {
        trace!("focus_prev");
        if self.has_focus() {
            self.widget_state.request_focus = Some(FocusChange::Previous);
        } else {
            warn!(
                "focus_prev can only be called by the currently \
                            focused widget or one of its ancestors."
            );
        }
    }

    /// Give up focus.
    ///
    /// This should only be called by a widget that currently has focus.
    ///
    /// See [`is_focused`](Self::is_focused) for more information about focus.
    pub fn resign_focus(&mut self) {
        trace!("resign_focus");
        if self.has_focus() {
            self.widget_state.request_focus = Some(FocusChange::Resign);
        } else {
            warn!(
                "resign_focus can only be called by the currently focused widget \
                 or one of its ancestors. ({:?})",
                self.widget_id()
            );
        }
    }
}

impl LifeCycleCtx<'_, '_> {
    /// Registers a child widget.
    ///
    /// This should only be called in response to a `LifeCycle::WidgetAdded` event.
    ///
    /// In general, you should not need to call this method; it is handled by
    /// the `WidgetPod`.
    // TODO - See issue #9
    pub(crate) fn register_child(&mut self, child_id: WidgetId) {
        trace!("register_child id={:?}", child_id);
        self.widget_state.children.add(&child_id);
    }

    /// Register this widget to be eligile to accept focus automatically.
    ///
    /// This should only be called in response to a [`LifeCycle::BuildFocusChain`] event.
    ///
    /// See [`EventCtx::is_focused`](Self::is_focused) for more information about focus.
    ///
    /// [`LifeCycle::BuildFocusChain`]: enum.Lifecycle.html#variant.BuildFocusChain
    pub fn register_for_focus(&mut self) {
        trace!("register_for_focus");
        self.widget_state.focus_chain.push(self.widget_id());
    }

    /// Register this widget as accepting text input.
    pub fn register_as_text_input(&mut self) {
        let registration = TextFieldRegistration {
            widget_id: self.widget_id(),
        };
        self.widget_state.text_registrations.push(registration);
    }

    // TODO - remove - See issue #15
    /// Register this widget as a portal.
    ///
    /// This should only be used by scroll areas.
    pub fn register_as_portal(&mut self) {
        self.widget_state.is_portal = true;
    }
}

impl LayoutCtx<'_, '_> {
    /// Set explicit paint [`Insets`] for this widget.
    ///
    /// You are not required to set explicit paint bounds unless you need
    /// to paint outside of your layout bounds. In this case, the argument
    /// should be an [`Insets`] struct that indicates where your widget
    /// needs to overpaint, relative to its bounds.
    ///
    /// For more information, see [`WidgetPod::paint_insets`].
    ///
    /// [`Insets`]: struct.Insets.html
    /// [`WidgetPod::paint_insets`]: struct.WidgetPod.html#method.paint_insets
    pub fn set_paint_insets(&mut self, insets: impl Into<Insets>) {
        let insets = insets.into();
        trace!("set_paint_insets {:?}", insets);
        self.widget_state.paint_insets = insets.nonnegative();
    }

    /// Set an explicit baseline position for this widget.
    ///
    /// The baseline position is used to align widgets that contain text,
    /// such as buttons, labels, and other controls. It may also be used
    /// by other widgets that are opinionated about how they are aligned
    /// relative to neighbouring text, such as switches or checkboxes.
    ///
    /// The provided value should be the distance from the *bottom* of the
    /// widget to the baseline.
    pub fn set_baseline_offset(&mut self, baseline: f64) {
        trace!("set_baseline_offset {}", baseline);
        self.widget_state.baseline_offset = baseline
    }

    /// Set the position of a child widget, in the paren't coordinate space. This
    /// will also implicitly change "hot" status and affect the parent's display rect.
    ///
    /// Container widgets must call this method with each non-stashed child in their
    /// layout method, after calling `child.layout(...)`.
    pub fn place_child(&mut self, child: &mut WidgetPod<impl Widget>, origin: Point) {
        child.state.origin = origin;
        child.state.is_expecting_place_child_call = false;
        let layout_rect = child.layout_rect();

        self.widget_state.local_paint_rect =
            self.widget_state.local_paint_rect.union(child.paint_rect());

        // if the widget has moved, it may have moved under the mouse, in which
        // case we need to handle that.
        if WidgetPod::update_hot_state(
            &mut child.inner,
            &mut child.state,
            self.global_state,
            layout_rect,
            self.mouse_pos,
        ) {
            self.widget_state.merge_up(&mut child.state);
        }
    }
}

impl PaintCtx<'_, '_> {
    /// The depth in the tree of the currently painting widget.
    ///
    /// This may be used in combination with [`paint_with_z_index`](Self::paint_with_z_index) in order
    /// to correctly order painting operations.
    ///
    /// The `depth` here may not be exact; it is only guaranteed that a child will
    /// have a greater depth than its parent.
    #[inline]
    pub fn depth(&self) -> u32 {
        self.depth
    }
}

impl<'a> GlobalPassCtx<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        ext_event_sink: ExtEventSink,
        debug_logger: &'a mut DebugLogger,
        command_queue: &'a mut CommandQueue,
        action_queue: &'a mut ActionQueue,
        timers: &'a mut HashMap<TimerToken, WidgetId>,
        mock_timer_queue: Option<&'a mut MockTimerQueue>,
        window: &'a WindowHandle,
        window_id: WindowId,
        focus_widget: Option<WidgetId>,
    ) -> Self {
        GlobalPassCtx {
            ext_event_sink,
            debug_logger,
            command_queue,
            action_queue,
            timers,
            mock_timer_queue,
            window,
            window_id,
            focus_widget,
            text: window.text(),
        }
    }

    pub(crate) fn submit_command(&mut self, command: Command) {
        trace!("submit_command");
        self.command_queue
            .push_back(command.default_to(self.window_id.into()));
    }

    pub(crate) fn submit_action(&mut self, action: Action, widget_id: WidgetId) {
        trace!("submit_action");
        self.action_queue
            .push_back((action, widget_id, self.window_id));
    }

    pub(crate) fn request_timer(&mut self, duration: Duration, widget_id: WidgetId) -> TimerToken {
        trace!("request_timer duration={:?}", duration);

        let timer_token = if let Some(timer_queue) = self.mock_timer_queue.as_mut() {
            // Path taken in unit tests, because we don't want to use platform timers
            timer_queue.add_timer(duration)
        } else {
            // Normal path
            self.window.request_timer(duration)
        };

        self.timers.insert(timer_token, widget_id);
        timer_token
    }
}
