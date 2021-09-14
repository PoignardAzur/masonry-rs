## TODO REFACTOR

- Re-read a ton of files

- Check invariants better
- Do some fuzzing
- Centralize event displaying
- Screenshots
- Widget: impl Debug
- Benchmarks


## ARCHITECTURE

The main communication between druid and druid-shell is in `druid-shell/src/window.rs`

`impl WindowHandle` is defined by the shell, and called by druid
(eg: `my_window_handle.set_title("Hello World")`)

`trait WinHandler` is defined by druid and called by the shell
(eg: `my_win_handler.paint(...)`)

Most of the non-plumbing logic is in `druid/src/core.rs` and `druid/src/window.rs`.

INVARIANT: when handling events, do the same things as this `AppState::do_window_event` in `druid/src/win_handler.rs`
(eg dispatch, then process_commands that might have been emitted, do_update, ime)