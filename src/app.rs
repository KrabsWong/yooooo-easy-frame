use crate::{
    export::{self, Environment},
    frame::{self, Frame},
};
use gpui_kit::{
    base::animation::{EffectTransition, ease_out_cubic},
    base::{StyledExt as _, h_flex, v_flex},
    assets::IconName as Lucide,
    component::{
        ActiveTheme as _, Disableable as _, Icon, Sizable as _, WindowExt as _,
        button::{Button, ButtonVariants as _},
        input::{Input, InputState},
        notification::Notification,
        spinner::Spinner,
        theme::Theme,
    },
    prelude::*,
    *,
};
use std::{
    cell::Cell,
    path::{Path as StdPath, PathBuf},
    rc::Rc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

actions!(apng_builder, [SelectPreviousFrame, SelectNextFrame]);

enum BootState {
    Checking,
    Ready(Environment),
    Failed(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DropPosition {
    Before,
    After,
}

#[derive(Clone, Copy)]
enum MoveDirection {
    Up,
    Down,
}

impl DropPosition {
    fn gap(self, row: usize) -> usize {
        match self {
            Self::Before => row,
            Self::After => row + 1,
        }
    }
}

pub struct Builder {
    boot: BootState,
    frames: Vec<Frame>,
    selected: Option<usize>,
    hovered_row: Option<usize>,
    dragging: Option<usize>,
    drop_target: Option<(usize, DropPosition)>,
    recent_move: Option<(usize, MoveDirection, u64)>,
    move_seq: u64,
    file_over: bool,
    exporting: Rc<Cell<bool>>,
    export_dialog_open: bool,
    interval_input: Option<Entity<InputState>>,
    quality_input: Option<Entity<InputState>>,
    _appearance_subscription: Option<Subscription>,
    _quit_subscription: Option<Subscription>,
    _visibility_subscription: Option<Subscription>,
    list_focus: FocusHandle,
    scroll_handle: ScrollHandle,
    preview_state: Entity<PreviewState>,
    preview_window: Option<AnyWindowHandle>,
    preview_request: Option<usize>,
    preview_seq: u64,
}

#[derive(Default)]
pub struct PreviewState {
    frame: Option<Frame>,
}

pub struct PreviewWindow {
    state: Entity<PreviewState>,
    _state_subscription: Subscription,
}

impl Render for PreviewWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(frame) = self.state.read(cx).frame.clone() else {
            return div().size_full().into_any_element();
        };
        v_flex()
            .size_full()
            .gap_3()
            .p_4()
            .bg(cx.theme().popover.opacity(0.9))
            .border_1()
            .border_color(cx.theme().border)
            .rounded_md()
            .child(
                img(frame.path.clone())
                    .w_full()
                    .h(px(200.))
                    .object_fit(ObjectFit::Contain),
            )
            .child(
                div()
                    .id("preview-info")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .v_flex()
                    .gap_1()
                    .children(preview_details(&frame, cx)),
            )
            .into_any_element()
    }
}

impl Builder {
    pub fn new(window: &mut Window, cx: &mut App) -> Entity<Self> {
        Theme::sync_system_appearance(Some(window), cx);
        let appearance_subscription = window.observe_window_appearance(|window, cx| {
            Theme::sync_system_appearance(Some(window), cx);
        });
        cx.bind_keys([
            KeyBinding::new("up", SelectPreviousFrame, Some("FrameList")),
            KeyBinding::new("down", SelectNextFrame, Some("FrameList")),
        ]);

        let list_focus = cx.focus_handle();
        let preview_state = cx.new(|_| PreviewState::default());
        let this = cx.new(|_| Self {
            boot: BootState::Checking,
            frames: Vec::new(),
            selected: None,
            hovered_row: None,
            dragging: None,
            drop_target: None,
            recent_move: None,
            move_seq: 0,
            file_over: false,
            exporting: Rc::new(Cell::new(false)),
            export_dialog_open: false,
            interval_input: None,
            quality_input: None,
            _appearance_subscription: Some(appearance_subscription),
            _quit_subscription: None,
            _visibility_subscription: None,
            list_focus,
            scroll_handle: ScrollHandle::new(),
            preview_state,
            preview_window: None,
            preview_request: None,
            preview_seq: 0,
        });

        let weak = this.downgrade();
        let quit_subscription = this.update(cx, |_, cx| {
            cx.on_app_quit(|this, cx| {
                this.close_preview(cx);
                async {}
            })
        });
        this.update(cx, |this, _| {
            this._quit_subscription = Some(quit_subscription);
        });

        window.on_window_should_close(cx, {
            let weak = weak.clone();
            move |window, cx| {
                let _ = weak.update(cx, |this, cx| this.close_preview(cx));
                window.minimize_window();
                false
            }
        });

        let visibility_subscription = this.update(cx, |this, cx| {
            this.window_visibility_subscription(window, cx)
        });
        this.update(cx, |this, _| {
            this._visibility_subscription = Some(visibility_subscription);
        });

        cx.spawn(async move |cx| {
            let result = cx
                .background_spawn(async {
                    std::thread::sleep(Duration::from_millis(220));
                    export::check_environment()
                })
                .await;
            let _ = weak.update(cx, |this, cx| {
                this.boot = match result {
                    Ok(environment) => BootState::Ready(environment),
                    Err(message) => BootState::Failed(message),
                };
                cx.notify();
            });
        })
        .detach();

        this
    }

    fn window_visibility_subscription(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Subscription {
        cx.observe_window_visibility(window, |this, visibility, _, cx| {
            if !visibility.is_visible() {
                this.close_preview(cx);
            }
        })
    }

    fn add_paths(&mut self, paths: Vec<PathBuf>) -> (usize, usize) {
        let mut added = 0;
        let mut skipped = 0;

        for path in paths {
            if path.is_file() && frame::is_png(&path) {
                match Frame::load(path) {
                    Ok(frame) => {
                        self.frames.push(frame);
                        added += 1;
                    }
                    Err(_) => skipped += 1,
                }
            } else {
                skipped += 1;
            }
        }

        if added > 0 && self.selected.is_none() {
            self.selected = Some(0);
        }
        (added, skipped)
    }

    fn handle_dropped_paths(
        &mut self,
        paths: &ExternalPaths,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (added, skipped) = self.add_paths(paths.paths().to_vec());
        if added > 0 {
            window.push_notification(
                Notification::success(format!("Added {added} PNG frames")),
                cx,
            );
        }
        if skipped > 0 {
            window.push_notification(
                Notification::warning(format!("Skipped {skipped} non-PNG or unreadable files")),
                cx,
            );
        }
        cx.notify();
    }

    fn open_file_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_preview(cx);
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Select PNG frames".into()),
        });
        let weak = cx.weak_entity();
        let window_handle = window.window_handle();

        cx.spawn(async move |_: WeakEntity<Self>, cx| {
            match receiver.await {
                Ok(Ok(Some(paths))) => {
                    let _ = weak.update(cx, |this, cx| {
                        let (added, skipped) = this.add_paths(paths);
                        let _ = cx.update_window(window_handle, |_, window, cx| {
                            if added > 0 {
                                window.push_notification(
                                    Notification::success(format!("Added {added} PNG frames")),
                                    cx,
                                );
                            }
                            if skipped > 0 {
                                window.push_notification(
                                    Notification::warning(format!(
                                        "Skipped {skipped} non-PNG or unreadable files"
                                    )),
                                    cx,
                                );
                            }
                        });
                        cx.notify();
                    });
                }
                Ok(Ok(None)) => {}
                Ok(Err(error)) => {
                    let _ = cx.update_window(window_handle, |_, window, cx| {
                        window.push_notification(
                            Notification::error(format!("Failed to open file picker: {error}")),
                            cx,
                        );
                    });
                }
                Err(error) => {
                    let _ = cx.update_window(window_handle, |_, window, cx| {
                        window.push_notification(
                            Notification::error(format!("Failed to open file picker: {error}")),
                            cx,
                        );
                    });
                }
            }
        })
        .detach();
    }

    fn move_frame(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        if from == to || from >= self.frames.len() || to > self.frames.len() {
            return;
        }
        let frame = self.frames.remove(from);
        let target = if from < to { to - 1 } else { to };
        self.frames.insert(target, frame);
        self.selected = Some(target);
        self.dragging = None;
        self.drop_target = None;
        self.hovered_row = None;
        self.close_preview(cx);

        self.move_seq = self.move_seq.wrapping_add(1);
        let token = self.move_seq;
        let direction = if target > from {
            MoveDirection::Down
        } else {
            MoveDirection::Up
        };
        self.recent_move = Some((target, direction, token));

        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            cx.background_spawn(async {
                std::thread::sleep(Duration::from_millis(420));
            })
            .await;
            let _ = weak.update(cx, |this, cx| {
                if this.recent_move.is_some_and(|(_, _, move_token)| move_token == token) {
                    this.recent_move = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn update_drop_target(&mut self, row: usize, position: Option<DropPosition>) -> bool {
        match position {
            Some(position) => {
                if self.drop_target != Some((row, position)) {
                    self.drop_target = Some((row, position));
                    true
                } else {
                    false
                }
            }
            None => {
                if self.drop_target.is_some_and(|(target, _)| target == row) {
                    self.drop_target = None;
                    true
                } else {
                    false
                }
            }
        }
    }

    fn select_relative(
        &mut self,
        delta: isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.frames.is_empty() {
            return;
        }
        let current = self.selected.unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, self.frames.len() as isize - 1) as usize;
        if next != self.selected.unwrap_or(0) {
            self.selected = Some(next);
            self.hovered_row = Some(next);
            self.scroll_handle.scroll_to_item(next);
            self.request_preview(Some(next), window, cx);
            cx.notify();
        }
    }

    pub fn close_preview(&mut self, cx: &mut Context<Self>) {
        self.preview_seq = self.preview_seq.wrapping_add(1);
        self.preview_request = None;
        if let Some(handle) = self.preview_window.take() {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
    }

    fn request_preview(
        &mut self,
        index: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = index.filter(|index| *index < self.frames.len()) else {
            self.close_preview(cx);
            return;
        };
        self.preview_seq = self.preview_seq.wrapping_add(1);
        let token = self.preview_seq;
        self.preview_request = Some(target);
        let Some(frame) = self.frames.get(target).cloned() else {
            return;
        };

        if self.preview_window.is_none() {
            self.show_preview(frame, window, cx);
            return;
        }

        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            cx.background_spawn(async {
                std::thread::sleep(Duration::from_millis(10));
            })
            .await;
            let _ = weak.update(cx, |this, cx| {
                if this.preview_seq != token || this.preview_request != Some(target) {
                    return;
                }
                let state = this.preview_state.clone();
                state.update(cx, |state, cx| {
                    state.frame = Some(frame);
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn show_preview(&mut self, frame: Frame, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.preview_state.clone();
        state.update(cx, |state, cx| {
            state.frame = Some(frame);
            cx.notify();
        });

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(preview_window_bounds(
                window.window_bounds().get_bounds(),
            ))),
            titlebar: None,
            show: true,
            focus: false,
            kind: WindowKind::Normal,
            is_movable: false,
            is_resizable: false,
            window_background: WindowBackgroundAppearance::Blurred,
            ..Default::default()
        };
        let handle = cx
            .open_window(options, |_, cx| {
                cx.new(|cx| {
                    let subscription = cx.observe(&state, |_, _, cx| cx.notify());
                    PreviewWindow {
                        state: state.clone(),
                        _state_subscription: subscription,
                    }
                })
            })
            .expect("failed to open preview window");
        self.preview_window = Some(handle.into());
    }
}

fn preview_window_bounds(main: Bounds<Pixels>) -> Bounds<Pixels> {
    let gap = px(0.);
    let width = px(360.);
    let height = px(520.);
    let left_x = main.origin.x - width - gap;
    let origin_x = if left_x >= px(0.) {
        left_x
    } else {
        main.origin.x + main.size.width + gap
    };
    Bounds::new(point(origin_x, main.origin.y), size(width, height))
}

impl Builder {

    fn delete_frame(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.frames.len() {
            return;
        }
        self.frames.remove(index);
        self.selected = if self.frames.is_empty() {
            None
        } else {
            Some(index.min(self.frames.len() - 1))
        };
        self.hovered_row = None;
        self.close_preview(cx);
        cx.notify();
    }

    fn clear_frames(&mut self, cx: &mut Context<Self>) {
        self.frames.clear();
        self.selected = None;
        self.hovered_row = None;
        self.dragging = None;
        self.drop_target = None;
        self.recent_move = None;
        self.hovered_row = None;
        self.close_preview(cx);
        cx.notify();
    }

    fn confirm_clear_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.frames.is_empty() {
            return;
        }
        let weak = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title("Clear all frames?")
                .width(px(390.))
                .child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(20.))
                        .child("All frames will be removed from the list."),
                )
                .footer(
                    h_flex()
                        .gap_2()
                        .justify_end()
                        .child(
                            Button::new("clear-cancel")
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child({
                            let weak = weak.clone();
                            Button::new("clear-confirm")
                                .label("Clear all")
                                .danger()
                                .on_click(move |_, window, cx| {
                                    let _ =
                                        weak.update(cx, |this, cx| this.clear_frames(cx));
                                    window.close_dialog(cx);
                                })
                        }),
                )
        });
    }

    fn confirm_delete(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(frame) = self.frames.get(index) else {
            return;
        };
        let name = frame.name.clone();
        let weak = cx.weak_entity();

        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title("Delete this image?")
                .width(px(390.))
                .child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(20.))
                        .child(format!("“{name}” will be removed from the frame list.")),
                )
                .footer(
                    h_flex()
                        .gap_2()
                        .justify_end()
                        .child(
                            Button::new("delete-cancel")
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child({
                            let weak = weak.clone();
                            Button::new("delete-confirm")
                                .label("Delete")
                                .danger()
                                .on_click(move |_, window, cx| {
                                    let _ = weak.update(cx, |this, cx| this.delete_frame(index, cx));
                                    window.close_dialog(cx);
                                })
                        }),
                )
        });
    }

    fn open_export_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.frames.is_empty() {
            window.push_notification(Notification::warning("Add PNG frames first"), cx);
            return;
        }
        self.hovered_row = None;
        self.close_preview(cx);
        self.export_dialog_open = true;
        cx.notify();

        let interval_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value("10")
                .placeholder("1-10000")
        });
        let quality_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value("80")
                .placeholder("1-100")
        });
        self.interval_input = Some(interval_input.clone());
        self.quality_input = Some(quality_input.clone());

        let weak = cx.weak_entity();
        let exporting = self.exporting.clone();
        let window_handle = window.window_handle();
        let frame_paths: Vec<PathBuf> = self.frames.iter().map(|frame| frame.path.clone()).collect();
        let first = &self.frames[0];
        let ffmpeg = match &self.boot {
            BootState::Ready(environment) => environment.ffmpeg.clone(),
            _ => PathBuf::from("ffmpeg"),
        };
        let default_dir = first
            .path
            .parent()
            .map(StdPath::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let default_name = first
            .path
            .file_stem()
            .map(|stem| format!("{}.apng", stem.to_string_lossy()))
            .unwrap_or_else(|| "animation.apng".into());
        let output_dimensions = format!("{} × {} px", first.info.width, first.info.height);
        let frame_count = self.frames.len();

        window.open_dialog(cx, move |dialog, _, cx| {
            let loading = exporting.get();
            dialog
                .title("Export APNG")
                .centered()
                .overlay(true)
                .overlay_closable(false)
                .close_button(false)
                .width(px(400.))
                .bg(cx.theme().popover.opacity(0.94))
                .child(
                    v_flex()
                        .gap_4()
                        .child(setting_field(
                            "Frame interval",
                            "Milliseconds, default 10 ms",
                            &interval_input,
                            cx,
                        ))
                        .child(setting_field(
                            "Quality",
                            "1-100; APNG is lossless, this controls PNG compression",
                            &quality_input,
                            cx,
                        ))
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "Output: {output_dimensions} · {frame_count} frames"
                                )),
                        )
                )
                .footer(
                    h_flex()
                        .gap_2()
                        .justify_end()
                        .child(
                            Button::new("export-cancel")
                                .label("Cancel")
                                .disabled(loading)
                                .on_click({
                                    let weak = weak.clone();
                                    move |_, window, cx| {
                                        let _ = weak.update(cx, |this, cx| {
                                            this.export_dialog_open = false;
                                            cx.notify();
                                        });
                                        window.close_dialog(cx);
                                    }
                                }),
                        )
                        .child({
                            let weak = weak.clone();
                            let exporting = exporting.clone();
                            let interval_input = interval_input.clone();
                            let quality_input = quality_input.clone();
                            let frame_paths = frame_paths.clone();
                            let default_dir = default_dir.clone();
                            let default_name = default_name.clone();
                                            let ffmpeg = ffmpeg.clone();

                            Button::new("export-confirm")
                                .label(if loading { "Exporting..." } else { "Export" })
                                .primary()
                                .loading(loading)
                                .disabled(loading)
                                .on_click(move |_, window, cx| {
                                    if exporting.get() {
                                        return;
                                    }

                                    let interval = interval_input.read(cx).value().trim().parse::<u16>();
                                    let quality = quality_input.read(cx).value().trim().parse::<u8>();
                                    let (Ok(interval), Ok(quality)) = (interval, quality) else {
                                        window.push_notification(
                                            Notification::error("Enter a valid interval and quality"),
                                            cx,
                                        );
                                        return;
                                    };
                                    if !(1..=10_000).contains(&interval) || !(1..=100).contains(&quality) {
                                        window.push_notification(
                                            Notification::error(
                                                "Interval must be 1-10000 ms and quality 1-100",
                                            ),
                                            cx,
                                        );
                                        return;
                                    }

                                    exporting.set(true);
                                    let _ = weak.update(cx, |_, cx| cx.notify());
                                    let receiver =
                                        cx.prompt_for_new_path(&default_dir, Some(&default_name));
                                    let weak = weak.clone();
                                    let frame_paths = frame_paths.clone();
                                                    let ffmpeg = ffmpeg.clone();
                                    let handle = window_handle;

                                    cx.spawn(async move |cx| {
                                        let output = match receiver.await {
                                            Ok(Ok(Some(path))) => path,
                                            Ok(Ok(None)) => {
                                                let _ = weak.update(cx, |this, cx| {
                                                    this.exporting.set(false);
                                                    this.export_dialog_open = false;
                                                    cx.notify();
                                                });
                                                return;
                                            }
                                            Ok(Err(error)) => {
                                                let _ = weak.update(cx, |this, cx| {
                                                    this.exporting.set(false);
                                                    this.export_dialog_open = false;
                                                    cx.notify();
                                                });
                                                let _ = cx.update_window(handle, |_, window, cx| {
                                                    window.push_notification(
                                                        Notification::error(format!(
                                                            "Failed to choose save path: {error}"
                                                        )),
                                                        cx,
                                                    );
                                                });
                                                return;
                                            }
                                            Err(error) => {
                                                let _ = weak.update(cx, |this, cx| {
                                                    this.exporting.set(false);
                                                    this.export_dialog_open = false;
                                                    cx.notify();
                                                });
                                                let _ = cx.update_window(handle, |_, window, cx| {
                                                    window.push_notification(
                                                        Notification::error(format!(
                                                            "Failed to choose save path: {error}"
                                                        )),
                                                        cx,
                                                    );
                                                });
                                                return;
                                            }
                                        };

                                        let result = cx
                                            .background_spawn({
                                                let frame_paths = frame_paths.clone();
                                                let output = output.clone();
                                                let ffmpeg = ffmpeg.clone();
                                                async move {
                                                    export::export_apng(
                                                        &ffmpeg,
                                                        &frame_paths,
                                                        &output,
                                                        interval,
                                                        quality,
                                                    )
                                                }
                                            })
                                            .await;

                                        let _ = weak.update(cx, |this, cx| {
                                            this.exporting.set(false);
                                            this.export_dialog_open = false;
                                            cx.notify();
                                        });
                                        let _ = cx.update_window(handle, |_, window, cx| {
                                            window.close_dialog(cx);
                                            match result {
                                                Ok(()) => window.push_notification(
                                                    Notification::success(format!(
                                                        "APNG exported to {}",
                                                        output.display()
                                                    )),
                                                    cx,
                                                ),
                                                Err(error) => window.push_notification(
                                                    Notification::error(error),
                                                    cx,
                                                ),
                                            }
                                        });
                                    })
                                    .detach();
                                })
                        }),
                )
        });
    }
}

impl Render for Builder {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (checking, failure, ready) = match &self.boot {
            BootState::Checking => (true, None, false),
            BootState::Failed(message) => (false, Some(message.clone()), false),
            BootState::Ready(_) => (false, None, true),
        };
        let content = if checking {
            self.render_checking(cx).into_any_element()
        } else if let Some(message) = failure {
            self.render_failure(&message, window, cx).into_any_element()
        } else if ready {
            self.render_ready(window, cx).into_any_element()
        } else {
            div().into_any_element()
        };
        let exporting = self.exporting.get();
        let scrim = self.export_dialog_open || exporting;

        v_flex()
            .size_full()
            .relative()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_action(cx.listener(|this, _: &crate::CloseWindow, window, cx| {
                this.close_preview(cx);
                window.minimize_window();
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|this, _: &crate::QuitApp, _, cx| {
                this.close_preview(cx);
                cx.quit();
                cx.stop_propagation();
            }))
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if !event.keystroke.modifiers.platform {
                    return;
                }
                match event.keystroke.key.as_str() {
                    "w" => {
                        this.close_preview(cx);
                        window.minimize_window();
                        cx.stop_propagation();
                    }
                    "q" => {
                        this.close_preview(cx);
                        cx.quit();
                        cx.stop_propagation();
                    }
                    _ => {}
                }
            }))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                this.file_over = false;
                this.handle_dropped_paths(paths, window, cx);
            }))
            .on_drag_move(cx.listener(|this, _: &DragMoveEvent<ExternalPaths>, _, cx| {
                if !this.file_over {
                    this.file_over = true;
                    cx.notify();
                }
            }))
            .on_file_drop_exit(cx.listener(|this, _, _, cx| {
                this.file_over = false;
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.dragging.take().is_some() || this.drop_target.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.dragging.take().is_some() || this.drop_target.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .child(self.render_toolbar(window, cx))
            .child(content)
            .when(scrim, |this| {
                this.child(
                    div()
                        .id("export-scrim")
                        .absolute()
                        .inset_0()
                        .bg(cx.theme().overlay.opacity(0.88))
                        .on_click(|_, _, cx| cx.stop_propagation()),
                )
            })
    }
}

impl Builder {
    fn render_toolbar(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let environment = match &self.boot {
            BootState::Ready(environment) => Some(format!("ffmpeg {}", environment.version)),
            _ => None,
        };

        h_flex()
            .h(px(48.))
            .min_h(px(48.))
            .w_full()
            .pl(px(84.))
            .pr(px(18.))
            .gap_4()
            .items_center()
            .justify_between()
            .bg(cx.theme().title_bar)
            .border_b_1()
            .border_color(cx.theme().title_bar_border)
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Icon::new(Lucide::Image)
                            .with_size(px(17.))
                            .text_color(cx.theme().foreground),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_medium()
                            .child("Easy Frame"),
                    )
            )
            .when_some(environment, |this, text| {
                this.child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            Icon::new(Lucide::CircleCheck)
                                .with_size(px(12.))
                                .text_color(cx.theme().success_foreground),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(cx.theme().muted_foreground)
                                .child(text),
                        ),
                )
            })
    }

    fn render_checking(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_4()
            .child(Spinner::new().with_size(px(22.)))
            .child(
                div()
                    .text_size(px(15.))
                    .font_medium()
                    .child("Checking local environment"),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(cx.theme().muted_foreground)
                    .child("Verifying ffmpeg installation and version"),
            )
    }

    fn render_failure(
        &mut self,
        message: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_4()
            .child(
                Icon::new(Lucide::TriangleAlert)
                    .with_size(px(28.))
                    .text_color(cx.theme().warning_foreground),
            )
            .child(
                div()
                    .text_size(px(18.))
                    .font_medium()
                    .child("Environment not ready"),
            )
            .child(
                div()
                    .max_w(px(460.))
                    .text_align(TextAlign::Center)
                    .text_size(px(13.))
                    .line_height(px(19.))
                    .text_color(cx.theme().muted_foreground)
                    .child(message.to_string()),
            )
            .child(
                div()
                    .mt_2()
                    .px_4()
                    .py_3()
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().muted)
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(20.))
                            .child("Install ffmpeg 5.0 or newer:\nbrew install ffmpeg\n\nThen restart Easy Frame."),
                    ),
            )
    }

    fn render_ready(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = div().flex_1().min_h_0();

        if self.frames.is_empty() {
            root.child(self.render_empty(window, cx))
        } else {
            root.child(self.render_sidebar(window, cx))
        }
    }

    fn render_empty(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                v_flex()
                    .w(px(340.))
                    .h(px(230.))
                    .gap_3()
                    .p_5()
                    .items_center()
                    .justify_center()
                    .rounded_lg()
                    .border_1()
                    .border_dashed()
                    .border_color(if self.file_over {
                        warm_accent()
                    } else {
                        cx.theme().border
                    })
                    .bg(if self.file_over {
                        cx.theme().drop_target.opacity(0.45)
                    } else {
                        cx.theme().muted.opacity(0.65)
                    })
                    .child(
                        Icon::new(Lucide::Images)
                            .with_size(px(28.))
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(
                        div()
                            .text_align(TextAlign::Center)
                            .text_size(px(16.))
                            .font_medium()
                            .child("Drop PNG frames into the window"),
                    )
                    .child(
                        div()
                            .text_align(TextAlign::Center)
                            .text_size(px(12.))
                            .text_color(cx.theme().muted_foreground)
                            .child("List order is the animation playback order"),
                    )
                    .child(
                        Button::new("add-images")
                            .label("Add Images")
                            .primary()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_file_picker(window, cx);
                            })),
                    ),
            )
    }

    fn render_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .w_full()
            .h_full()
            .min_h_0()
            .relative()
            .key_context("FrameList")
            .track_focus(&self.list_focus)
            .on_action(cx.listener(|this, _: &SelectPreviousFrame, window, cx| {
                this.select_relative(-1, window, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectNextFrame, window, cx| {
                this.select_relative(1, window, cx);
            }))
            .bg(cx.theme().sidebar)
            .child(
                h_flex()
                    .h(px(40.))
                    .min_h(px(40.))
                    .px_3()
                    .items_center()
                    .justify_between()
                    .bg(cx.theme().list_head)
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_medium()
                            .child(format!("Frames ({})", self.frames.len())),
                    )
                    .child(
                        div()
                            .id("clear-all")
                            .w(px(26.))
                            .h(px(26.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_sm()
                            .bg(cx.theme().muted.opacity(0.7))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.confirm_clear_all(window, cx);
                            }))
                            .child(
                                Icon::new(Lucide::Trash)
                                    .with_size(px(14.))
                                    .text_color(cx.theme().foreground),
                            ),
                    ),
            )
            .child(
                div()
                    .id("frame-list")
                    .track_scroll(&self.scroll_handle)
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_1()
                    .pb(px(64.))
                    .v_flex()
                    .gap_1()
                    .on_hover(cx.listener(|this, hovering, _, cx| {
                        if !hovering {
                            this.close_preview(cx);
                        }
                    }))
                    .children({
                        let frames = self.frames.clone();
                        let mut rows: Vec<AnyElement> = Vec::with_capacity(frames.len());
                        for (index, frame) in frames.into_iter().enumerate() {
                            rows.push(self.render_row(index, frame, window, cx).into_any_element());
                        }
                        rows
                    }),
            )
            .child(
                div()
                    .absolute()
                    .bottom(px(16.))
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .gap_2()
                    .child(
                        pill_button("add-more", "Add Images", false, cx)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_file_picker(window, cx);
                            })),
                    )
                    .child(
                        pill_button("export", "Export", true, cx)
                            .disabled(self.frames.is_empty())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_export_dialog(window, cx);
                            })),
                    ),
            )
    }

    fn render_row(
        &mut self,
        index: usize,
        frame: Frame,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.selected == Some(index);
        let hovered = self.hovered_row == Some(index);
        let is_dragging = self.dragging == Some(index);
        let drop_position = self
            .drop_target
            .filter(|(target, _)| *target == index && cx.has_active_drag())
            .map(|(_, position)| position);
        let recent_move = self
            .recent_move
            .filter(|(moved, _, _)| *moved == index);
        let name = frame.name.clone();
        let weak = cx.weak_entity();

        let row = div()
            .id(("frame-row", index))
            .relative()
            .h(px(60.))
            .min_h(px(60.))
            .px_2()
            .gap_2()
            .flex()
            .items_center()
            .rounded_sm()
            .bg(if selected {
                selected_row_bg()
            } else if hovered {
                hover_row_bg()
            } else {
                cx.theme().sidebar
            })
            .when(is_dragging, |this| this.opacity(0.45))
            .drag_over::<usize>(|style, _, _, _| style.bg(warm_accent_soft()))
            .on_hover(cx.listener(move |this, hovering, window, cx| {
                if *hovering {
                    if this.hovered_row != Some(index) {
                        this.hovered_row = Some(index);
                        this.request_preview(Some(index), window, cx);
                        cx.notify();
                    }
                } else if this.hovered_row == Some(index) {
                    this.hovered_row = None;
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.list_focus.focus(window, cx);
                if this.selected != Some(index) {
                    this.selected = Some(index);
                    cx.notify();
                }
            }))
            .on_drag(index, move |_, _, _, cx| {
                let _ = weak.update(cx, |this, cx| {
                    this.dragging = Some(index);
                    this.drop_target = None;
                    cx.notify();
                });
                cx.new(|_| DragPreview {
                    name: name.clone(),
                })
            })
            .on_drag_move(cx.listener(move |this, event: &DragMoveEvent<usize>, _, cx| {
                let from = *event.drag(cx);
                let position = if event.bounds.contains(&event.event.position) {
                    let position = if event.event.position.y < event.bounds.center().y {
                        DropPosition::Before
                    } else {
                        DropPosition::After
                    };
                    let gap = position.gap(index);
                    if gap == from || gap == from + 1 {
                        None
                    } else {
                        Some(position)
                    }
                } else {
                    None
                };
                if this.dragging != Some(from) {
                    this.dragging = Some(from);
                }
                if this.update_drop_target(index, position) {
                    cx.notify();
                }
            }))
            .on_drop(cx.listener(move |this, from: &usize, _, cx| {
                let position = this
                    .drop_target
                    .filter(|(target, _)| *target == index)
                    .map(|(_, position)| position);
                if let Some(position) = position {
                    this.move_frame(*from, position.gap(index), cx);
                } else {
                    this.dragging = None;
                    this.drop_target = None;
                }
                cx.notify();
            }))
            .when_some(drop_position, |this, position| {
                let line = render_drop_indicator(cx);
                this.child(match position {
                    DropPosition::Before => line.top(px(-1.)),
                    DropPosition::After => line.bottom(px(-1.)),
                })
            })
            .when_some(recent_move, |this, (_, _, token)| {
                this.child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded_sm()
                        .bg(warm_accent_soft())
                        .with_animation(
                            ("frame-move-glow", token),
                            Animation::new(Duration::from_millis(420))
                                .with_easing(ease_out_cubic),
                            |this, progress| this.opacity(1.0 - progress),
                        ),
                )
            })
            .child(
                div()
                    .w(px(42.))
                    .h(px(42.))
                    .min_w(px(42.))
                    .rounded_sm()
                    .overflow_hidden()
                    .bg(cx.theme().muted)
                    .child(
                        img(frame.path.clone())
                            .w_full()
                            .h_full()
                            .object_fit(ObjectFit::Contain),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(
                        div()
                            .w_full()
                            .truncate()
                            .text_size(px(13.))
                            .font_medium()
                            .child(elide(&frame.name, 24)),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .overflow_x_hidden()
                            .child(tag(format!("Format: PNG"), cx))
                            .child(tag(
                                format!("Pixels: {}×{}", frame.info.width, frame.info.height),
                                cx,
                            ))
                            .child(tag(format!("Size: {}", format_bytes(frame.size_bytes)), cx)),
                    ),
            )
            .child(
                div()
                    .id(("delete-frame", index))
                    .w(px(24.))
                    .h(px(24.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_sm()
                    .bg(if hovered {
                        cx.theme().muted
                    } else {
                        cx.theme().muted.opacity(0.7)
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.confirm_delete(index, window, cx);
                    }))
                    .child(
                        Icon::new(Lucide::Trash)
                            .with_size(px(14.))
                            .text_color(if hovered {
                                cx.theme().foreground
                            } else {
                                cx.theme().foreground
                            }),
                    ),
            )
            ;

        let row_element: AnyElement = if let Some((_, direction, token)) = recent_move {
            let from = match direction {
                MoveDirection::Up => px(8.),
                MoveDirection::Down => px(-8.),
            };
            EffectTransition::new(Duration::from_millis(280))
                .slide_y(from, px(0.))
                .fade(0.72, 1.0)
                .apply(row, ("frame-move", token))
                .into_any_element()
        } else {
            row.into_any_element()
        };

        row_element
    }

    #[allow(dead_code)]
    fn render_detail(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(index) = self.selected else {
            return div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(cx.theme().muted_foreground)
                        .child("Select a frame to view details"),
                )
                .into_any_element();
        };
        let Some(frame) = self.frames.get(index) else {
            return div().flex_1().into_any_element();
        };

        let info = &frame.info;
        let mut details = vec![
            ("Name", frame.name.clone()),
            ("Path", frame.path.display().to_string()),
            (
                "Size",
                format!("{} ({} bytes)", format_bytes(frame.size_bytes), frame.size_bytes),
            ),
            (
                "Dimensions",
                format!("{} × {} px", info.width, info.height),
            ),
            ("Format", format!("PNG · bit depth {}", info.bit_depth)),
            ("Color type", info.color_type_name().to_string()),
            (
                "Compression",
                format!("deflate / filter {} / interlace {}", info.compression, info.filter),
            ),
            ("Interlace", info.interlace_name().to_string()),
            (
                "DPI",
                match (info.dpi_x, info.dpi_y) {
                    (Some(x), Some(y)) => format!("{x:.1} × {y:.1}"),
                    _ => "Not recorded".into(),
                },
            ),
            (
                "Gamma",
                info.gamma
                    .map(|value| format!("{value:.2}"))
                    .unwrap_or_else(|| "Not recorded".into()),
            ),
            (
                "sRGB intent",
                info.srgb_intent
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "Not recorded".into()),
            ),
            (
                "Modified",
                frame
                    .modified
                    .and_then(format_system_time)
                    .unwrap_or_else(|| "Not recorded".into()),
            ),
            (
                "PNG chunks",
                if info.chunks.is_empty() {
                    "None".into()
                } else {
                    info.chunks.join(" · ")
                },
            ),
        ];
        if !info.text_entries.is_empty() {
            let text = info
                .text_entries
                .iter()
                .take(8)
                .map(|(key, value)| format!("{key}={value}"))
                .collect::<Vec<_>>()
                .join("\n");
            details.push(("Text metadata", text));
        }
        let split = (details.len() + 1) / 2;
        let left_details: Vec<_> = details[..split]
            .iter()
            .map(|(label, value)| compact_detail_row(label, value.clone(), cx))
            .collect();
        let right_details: Vec<_> = details[split..]
            .iter()
            .map(|(label, value)| compact_detail_row(label, value.clone(), cx))
            .collect();

        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .min_h_0()
            .bg(cx.theme().background)
            .child(
                h_flex()
                    .h(px(44.))
                    .min_h(px(44.))
                    .px_4()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .truncate()
                            .text_size(px(15.))
                            .font_medium()
                            .child(frame.name.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("Frame {} of {}", index + 1, self.frames.len())),
                    ),
            )
            .child(
                div()
                    .id("detail-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_4()
                    .child(
                        v_flex()
                            .w_full()
                            .gap_4()
                            .child(
                                div()
                                    .h(px(280.))
                                    .min_h(px(220.))
                                    .w_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(cx.theme().border)
                                    .bg(cx.theme().muted)
                                    .child(
                                        img(frame.path.clone())
                                            .w_full()
                                            .h_full()
                                            .object_fit(ObjectFit::Contain),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .w_full()
                                    .gap_6()
                                    .items_start()
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w_0()
                                            .gap_1()
                                            .children(left_details),
                                    )
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w_0()
                                            .gap_1()
                                            .children(right_details),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }
}

struct DragPreview {
    name: String,
}

impl Render for DragPreview {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .max_w(px(240.))
            .truncate()
            .rounded_md()
            .bg(cx.theme().accent)
            .px_3()
            .py_2()
            .text_size(px(13.))
            .text_color(cx.theme().accent_foreground)
            .shadow_md()
            .child(format!("Move {}", elide(&self.name, 24)))
    }
}

fn setting_field(
    label: &str,
    hint: &str,
    input: &Entity<InputState>,
    cx: &App,
) -> impl IntoElement {
    v_flex()
        .gap_1()
        .child(
            h_flex()
                .w_full()
                .gap_3()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(13.))
                        .font_medium()
                        .child(label.to_string()),
                )
                .child(Input::new(input).w(px(150.))),
        )
        .child(
            div()
                .text_size(px(12.))
                .text_color(cx.theme().muted_foreground)
                .child(hint.to_string()),
        )
}

fn preview_details(frame: &Frame, cx: &App) -> Vec<AnyElement> {
    let info = &frame.info;
    let mut details = vec![
        ("Name", frame.name.clone()),
        ("Path", frame.path.display().to_string()),
        (
            "File size",
            format!("{} ({} bytes)", format_bytes(frame.size_bytes), frame.size_bytes),
        ),
        (
            "Dimensions",
            format!("{} × {} px", info.width, info.height),
        ),
        ("Format", "PNG".into()),
        ("Bit depth", info.bit_depth.to_string()),
        ("Color type", info.color_type_name().to_string()),
        (
            "Compression",
            format!("deflate, filter {}, interlace {}", info.compression, info.filter),
        ),
        ("Interlace", info.interlace_name().to_string()),
        (
            "DPI",
            match (info.dpi_x, info.dpi_y) {
                (Some(x), Some(y)) => format!("{x:.1} × {y:.1}"),
                _ => "Not recorded".into(),
            },
        ),
        (
            "Gamma",
            info.gamma
                .map(|value| format!("{value:.2}"))
                .unwrap_or_else(|| "Not recorded".into()),
        ),
        (
            "sRGB intent",
            info.srgb_intent
                .map(|value| value.to_string())
                .unwrap_or_else(|| "Not recorded".into()),
        ),
        (
            "Modified",
            frame
                .modified
                .and_then(format_system_time)
                .unwrap_or_else(|| "Not recorded".into()),
        ),
        (
            "PNG chunks",
            if info.chunks.is_empty() {
                "None".into()
            } else {
                info.chunks.join(" · ")
            },
        ),
    ];
    if !info.text_entries.is_empty() {
        details.push((
            "Text metadata",
            info.text_entries
                .iter()
                .take(8)
                .map(|(key, value)| format!("{key}={value}"))
                .collect::<Vec<_>>()
                .join("\n"),
        ));
    }
    details
        .into_iter()
        .map(|(label, value)| compact_detail_row(label, value, cx).into_any_element())
        .collect()
}

fn selected_row_bg() -> gpui_kit::Hsla {
    gpui_kit::hsla(0.07, 0.82, 0.55, 0.24)
}

fn hover_row_bg() -> gpui_kit::Hsla {
    gpui_kit::hsla(0.09, 0.72, 0.62, 0.14)
}

fn warm_accent() -> gpui_kit::Hsla {
    gpui_kit::hsla(0.07, 0.82, 0.50, 1.0)
}

fn warm_accent_soft() -> gpui_kit::Hsla {
    gpui_kit::hsla(0.07, 0.82, 0.50, 0.20)
}

fn render_drop_indicator(_cx: &App) -> Div {
    div()
        .absolute()
        .left_0()
        .right_0()
        .h(px(2.))
        .rounded_full()
        .bg(warm_accent())
}

fn pill_button(id: &'static str, label: &'static str, primary: bool, cx: &App) -> Button {
    let button = Button::new(id)
        .label(label)
        .rounded_full()
        .h(px(34.))
        .min_h(px(34.))
        .py(px(0.))
        .px(px(18.))
        .shadow_md();
    if primary {
        button.primary()
    } else {
        button
            .ghost()
            .bg(cx.theme().background.opacity(0.72))
            .border_1()
            .border_color(cx.theme().border.opacity(0.9))
    }
}

fn compact_detail_row(label: &str, value: String, cx: &App) -> impl IntoElement {
    h_flex()
        .w_full()
        .gap_2()
        .py_1()
        .items_start()
        .child(
            div()
                .w(px(88.))
                .min_w(px(88.))
                .text_size(px(12.))
                .text_color(cx.theme().muted_foreground)
                .child(label.to_string()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(12.))
                .line_height(px(17.))
                .child(value),
        )
}

fn tag(text: impl Into<SharedString>, cx: &App) -> impl IntoElement {
    div()
        .rounded_sm()
        .bg(cx.theme().muted)
        .px_1()
        .py_0()
        .text_size(px(11.))
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

fn elide(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    let mut result: String = text.chars().take(max_chars.saturating_sub(3)).collect();
    result.push_str("...");
    result
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / 1024.0 / 1024.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

#[allow(dead_code)]
fn format_system_time(time: SystemTime) -> Option<String> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| format!("{} (Unix seconds)", duration.as_secs()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn drop_position_resolves_insertion_gap() {
        assert_eq!(DropPosition::Before.gap(3), 3);
        assert_eq!(DropPosition::After.gap(3), 4);
    }
}
