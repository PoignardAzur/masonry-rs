use crate::testing::{
    widget_ids, Harness, ModularWidget, Record, Recording, ReplaceChild, TestWidgetExt as _,
    REPLACE_CHILD,
};
use crate::widget::{Flex, Label, SizedBox};
use crate::*;
use smallvec::smallvec;
use std::cell::Cell;
use std::rc::Rc;
use test_env_log::test;

const REQUEST_FOCUS: Selector<()> = Selector::new("druid-tests.request-focus");

struct FocusTaker;

impl FocusTaker {
    fn new() -> impl Widget {
        Self::track(Default::default())
    }

    fn track(focused: Rc<Cell<bool>>) -> impl Widget {
        ModularWidget::new(focused)
            .event_fn(|_is_focused, ctx, event, env| {
                if let Event::Command(cmd) = event {
                    if cmd.is(REQUEST_FOCUS) {
                        ctx.request_focus();
                        return;
                    }
                }
            })
            .status_change_fn(|is_focused, _ctx, event, env| {
                if let StatusChange::FocusChanged(focus) = event {
                    is_focused.set(*focus);
                }
            })
            .lifecycle_fn(|_is_focused, ctx, event, env| {
                if let LifeCycle::BuildFocusChain = event {
                    ctx.register_for_focus();
                }
            })
    }
}

/// Check that a focus chain is correctly built initially..
#[test]
fn build_focus_chain() {
    let [id_1, id_2, id_3, id_4] = widget_ids();

    let widget = Flex::column()
        .with_child_id(FocusTaker::new(), id_1)
        .with_child_id(FocusTaker::new(), id_2)
        .with_child_id(FocusTaker::new(), id_3)
        .with_child_id(FocusTaker::new(), id_4);

    let mut harness = Harness::create(widget);

    // verify that we start out with four widgets registered for focus
    assert_eq!(harness.window().focus_chain(), &[id_1, id_2, id_3, id_4]);
}

/// Check that focus changes trigger on_status_change
#[test]
fn focus_status_change() {
    let [id_1, id_2] = widget_ids();

    // we use these so that we can check that on_status_check was called
    let left_focus: Rc<Cell<bool>> = Default::default();
    let right_focus: Rc<Cell<bool>> = Default::default();
    assert_eq!(left_focus.get(), false);
    assert_eq!(right_focus.get(), false);

    let widget = Flex::row()
        .with_child_id(FocusTaker::track(left_focus.clone()), id_1)
        .with_child_id(FocusTaker::track(right_focus.clone()), id_2);

    let mut harness = Harness::create(widget);

    // nobody should have focus
    assert_eq!(left_focus.get(), false);
    assert_eq!(right_focus.get(), false);

    harness.submit_command(REQUEST_FOCUS.to(id_1));
    // check that left widget got "on_status_change" event.
    assert_eq!(left_focus.get(), true);
    assert_eq!(right_focus.get(), false);

    harness.submit_command(REQUEST_FOCUS.to(id_2));
    // check that left and right widget got "on_status_change" event.
    assert_eq!(left_focus.get(), false);
    assert_eq!(right_focus.get(), true);
}

/// test that the last widget to request focus during an event gets it.
#[test]
fn take_focus() {
    let [id_1, id_2, id_3, id_4] = widget_ids();

    let widget = Flex::row()
        .with_child_id(FocusTaker::new(), id_1)
        .with_child_id(FocusTaker::new(), id_2)
        .with_child_id(FocusTaker::new(), id_3)
        .with_child_id(FocusTaker::new(), id_4);

    let mut harness = Harness::create(widget);

    // nobody should have focus
    assert_eq!(harness.window().focus, None);

    // this is sent to all widgets; the last widget to request focus should get it
    harness.submit_command(REQUEST_FOCUS);
    assert_eq!(harness.window().focus, Some(id_4));

    // this is sent to all widgets; the last widget to request focus should still get it
    harness.submit_command(REQUEST_FOCUS);
    assert_eq!(harness.window().focus, Some(id_4));
}

#[test]
fn focus_updated_by_children_change() {
    let [id_1, id_2, id_3, id_4, id_5, id_6] = widget_ids();

    // this widget starts with a single child, and will replace them with a split
    // when we send it a command.
    let replacer = ReplaceChild::new(FocusTaker::new().with_id(id_4), move || {
        Flex::row()
            .with_child_id(FocusTaker::new(), id_5)
            .with_child_id(FocusTaker::new(), id_6)
    });

    let widget = Flex::row()
        .with_child_id(FocusTaker::new(), id_1)
        .with_child_id(FocusTaker::new(), id_2)
        .with_child_id(FocusTaker::new(), id_3)
        .with_child(replacer);

    let mut harness = Harness::create(widget);

    // verify that we start out with four widgets registered for focus
    assert_eq!(harness.window().focus_chain(), &[id_1, id_2, id_3, id_4]);

    // tell the replacer widget to swap its children
    harness.submit_command(REPLACE_CHILD);

    // verify that the two new children are registered for focus.
    assert_eq!(
        harness.window().focus_chain(),
        &[id_1, id_2, id_3, id_5, id_6]
    );
}

// FIXME
#[test]
#[ignore]
fn resign_focus_on_disable() {
    const CHANGE_DISABLED: Selector<bool> = Selector::new("druid-tests.change-disabled");

    fn make_container_widget(id: WidgetId, child: impl Widget) -> impl Widget {
        ModularWidget::new(WidgetPod::new_with_id(child, id))
            .event_fn(|child, ctx, event, env| {
                if let Event::Command(cmd) = event {
                    if let Some(disabled) = cmd.try_get(CHANGE_DISABLED) {
                        ctx.set_disabled(*disabled);
                        // TODO
                        //return;
                    }
                }
                child.on_event(ctx, event, env);
            })
            .lifecycle_fn(|child, ctx, event, env| {
                child.lifecycle(ctx, event, env);
            })
            .children_fns(
                |child| smallvec![child as &dyn AsWidgetPod],
                |child| smallvec![child as &mut dyn AsWidgetPod],
            )
    }

    let [id_0, id_1, id_2] = widget_ids();

    let root = Flex::row()
        .with_child(make_container_widget(
            id_0,
            make_container_widget(id_1, FocusTaker::new()),
        ))
        .with_child(make_container_widget(id_2, FocusTaker::new()));

    let mut harness = Harness::create(root);
    assert_eq!(harness.window().focus_chain(), &[id_0, id_1, id_2]);
    assert_eq!(harness.window().focus, None);

    harness.submit_command(REQUEST_FOCUS.to(id_2));
    assert_eq!(harness.window().focus_chain(), &[id_0, id_1, id_2]);
    assert_eq!(harness.window().focus, Some(id_2));

    harness.submit_command(CHANGE_DISABLED.with(true).to(id_0));
    assert_eq!(harness.window().focus_chain(), &[id_2]);
    assert_eq!(harness.window().focus, Some(id_2));

    harness.submit_command(CHANGE_DISABLED.with(true).to(id_2));
    assert_eq!(harness.window().focus_chain(), &[]);
    assert_eq!(harness.window().focus, None);

    harness.submit_command(CHANGE_DISABLED.with(false).to(id_0));
    assert_eq!(harness.window().focus_chain(), &[id_0, id_1]);
    assert_eq!(harness.window().focus, None);

    harness.submit_command(REQUEST_FOCUS.to(id_1));
    assert_eq!(harness.window().focus_chain(), &[id_0, id_1]);
    assert_eq!(harness.window().focus, Some(id_1));

    harness.submit_command(CHANGE_DISABLED.with(false).to(id_2));
    assert_eq!(harness.window().focus_chain(), &[id_0, id_1, id_2]);
    assert_eq!(harness.window().focus, Some(id_1));

    harness.submit_command(CHANGE_DISABLED.with(true).to(id_0));
    assert_eq!(harness.window().focus_chain(), &[id_2]);
    assert_eq!(harness.window().focus, None);
}