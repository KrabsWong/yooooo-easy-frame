use crate::{
    export::{self, Environment},
    frame::{self, Frame},
    settings::{Language, Settings},
    updater,
};
use gpui_kit::{
    base::animation::{EffectTransition, ease_out_cubic},
    base::{StyledExt as _, h_flex, v_flex},
    assets::IconName as Lucide,
    component::{
        ActiveTheme as _, Disableable as _, Icon, Sizable as _, WindowExt as _,
        button::{Button, ButtonVariants as _},
        input::{Input, InputEvent, InputState},
        notification::Notification,
        select::{SearchableVec, Select, SelectEvent, SelectState},
        spinner::Spinner,
        tab::{Tab, TabBar},
        theme::Theme,
    },
    base::IndexPath,
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

const EMPTY_STATE_WATERMARK: &[u8] = include_bytes!("../resources/easy-frame-watermark.svg");

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

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    General,
    About,
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
    filename_input: Option<Entity<InputState>>,
    export_subscriptions: Vec<Subscription>,
    settings_subscription: Option<Subscription>,
    _appearance_subscription: Option<Subscription>,
    _quit_subscription: Option<Subscription>,
    _visibility_subscription: Option<Subscription>,
    list_focus: FocusHandle,
    scroll_handle: ScrollHandle,
    preview_state: Entity<PreviewState>,
    preview_window: Option<AnyWindowHandle>,
    preview_request: Option<usize>,
    last_preview_index: Option<usize>,
    preview_seq: u64,
    language: Language,
}

#[derive(Default)]
pub struct PreviewState {
    frame: Option<Frame>,
}

pub struct PreviewWindow {
    state: Entity<PreviewState>,
    _state_subscription: Subscription,
    language: Language,
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
            .children(preview_details(&frame, self.language, cx)),
            )
            .into_any_element()
    }
}

impl Builder {
    pub fn new(window: &mut Window, cx: &mut App) -> Entity<Self> {
        sync_theme(Some(window), cx);
        let appearance_subscription = window.observe_window_appearance(|window, cx| {
            sync_theme(Some(window), cx);
        });
        cx.bind_keys([
            KeyBinding::new("up", SelectPreviousFrame, Some("FrameList")),
            KeyBinding::new("down", SelectNextFrame, Some("FrameList")),
        ]);

        let list_focus = cx.focus_handle();
        let preview_state = cx.new(|_| PreviewState::default());
        let language = Settings::load().language;
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
            filename_input: None,
            export_subscriptions: Vec::new(),
            settings_subscription: None,
            _appearance_subscription: Some(appearance_subscription),
            _quit_subscription: None,
            _visibility_subscription: None,
            list_focus,
            scroll_handle: ScrollHandle::new(),
            preview_state,
            preview_window: None,
            preview_request: None,
            last_preview_index: None,
            preview_seq: 0,
            language,
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
                Notification::success(added_frames_notice(self.language, added)),
                cx,
            );
        }
        if skipped > 0 {
            window.push_notification(
                Notification::warning(skipped_frames_notice(self.language, skipped)),
                cx,
            );
        }
        cx.notify();
    }

    fn open_file_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_preview(cx);
        let lang = self.language;
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some(t(self.language, "Select PNG frames", "选择 PNG 图片").into()),
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
                                    Notification::success(added_frames_notice(this.language, added)),
                                    cx,
                                );
                            }
                            if skipped > 0 {
                                window.push_notification(
                                    Notification::warning(skipped_frames_notice(this.language, skipped)),
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
                                                Notification::error(picker_error(lang, error)),
                            cx,
                        );
                    });
                }
                Err(error) => {
                    let _ = cx.update_window(window_handle, |_, window, cx| {
                        window.push_notification(
                                                Notification::error(picker_error(lang, error)),
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
        self.last_preview_index = Some(target);
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
        self.last_preview_index = Some(target);
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
        let language = self.language;
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
                        language,
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
        self.last_preview_index = self.selected;
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
        self.last_preview_index = None;
        self.close_preview(cx);
        cx.notify();
    }

    fn confirm_clear_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.frames.is_empty() {
            return;
        }
        let weak = cx.weak_entity();
        let lang = self.language;
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title(t(lang, "Clear all frames?", "清空所有图片？"))
                .width(px(390.))
                .child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(20.))
                        .child(t(
                            lang,
                            "All frames will be removed from the list.",
                            "所有图片都会从列表中移除。",
                        )),
                )
                .footer(
                    h_flex()
                        .gap_2()
                        .justify_end()
                        .child(
                            Button::new("clear-cancel")
                                .label(t(lang, "Cancel", "取消"))
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child({
                            let weak = weak.clone();
                            Button::new("clear-confirm")
                                .label(t(lang, "Clear all", "全部清空"))
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
        let lang = self.language;

        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title(t(lang, "Delete this image?", "删除这张图片？"))
                .width(px(390.))
                .child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(20.))
                        .child(delete_notice(lang, &name)),
                )
                .footer(
                    h_flex()
                        .gap_2()
                        .justify_end()
                        .child(
                            Button::new("delete-cancel")
                                .label(t(lang, "Cancel", "取消"))
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child({
                            let weak = weak.clone();
                            Button::new("delete-confirm")
                                .label(t(lang, "Delete", "删除"))
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
            window.push_notification(
                Notification::warning(t(
                    self.language,
                    "Add PNG frames first",
                    "请先添加 PNG 图片",
                )),
                cx,
            );
            return;
        }
        let lang = self.language;
        self.hovered_row = None;
        self.close_preview(cx);
        self.export_dialog_open = true;
        self.export_subscriptions.clear();
        cx.notify();

        let first = &self.frames[0];
        let default_dir = first
            .path
            .parent()
            .map(StdPath::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let default_stem = first
            .path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "animation".into());
        let output_dimensions = format!("{} × {} px", first.info.width, first.info.height);
        let frame_count = self.frames.len();

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
        let filename_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(format!("{default_stem}-10-80.png"))
                .placeholder("animation-10-80.png")
        });
        self.interval_input = Some(interval_input.clone());
        self.quality_input = Some(quality_input.clone());
        self.filename_input = Some(filename_input.clone());

        let weak = cx.weak_entity();
        let exporting = self.exporting.clone();
        let main_window: AnyWindowHandle = window.window_handle().into();
        let frame_paths: Vec<PathBuf> = self.frames.iter().map(|frame| frame.path.clone()).collect();
        let ffmpeg = match &self.boot {
            BootState::Ready(environment) => environment.ffmpeg.clone(),
            _ => PathBuf::from("ffmpeg"),
        };
        let filename_edited = Rc::new(Cell::new(false));

        let interval_for_name = interval_input.clone();
        let quality_for_name = quality_input.clone();
        let interval_filename = filename_input.clone();
        let interval_edited = filename_edited.clone();
        let interval_window = main_window;
        let interval_stem = default_stem.clone();
        let interval_subscription = cx.subscribe(&interval_input, move |_, _, event: &InputEvent, cx| {
            if !matches!(event, InputEvent::Change) || interval_edited.get() {
                return;
            }
            let name = suggested_export_filename(&interval_stem, &interval_for_name, &quality_for_name, cx);
            let _ = cx.update_window(interval_window, |_, window, cx| {
                interval_filename.update(cx, |input, cx| input.set_value(name, window, cx));
            });
        });

        let quality_for_name = quality_input.clone();
        let interval_for_quality = interval_input.clone();
        let quality_filename = filename_input.clone();
        let quality_edited = filename_edited.clone();
        let quality_window = main_window;
        let quality_stem = default_stem.clone();
        let quality_subscription = cx.subscribe(&quality_input, move |_, _, event: &InputEvent, cx| {
            if !matches!(event, InputEvent::Change) || quality_edited.get() {
                return;
            }
            let name = suggested_export_filename(&quality_stem, &interval_for_quality, &quality_for_name, cx);
            let _ = cx.update_window(quality_window, |_, window, cx| {
                quality_filename.update(cx, |input, cx| input.set_value(name, window, cx));
            });
        });

        let quality_guard = quality_input.clone();
        let quality_guard_window = main_window;
        let quality_guard_subscription = cx.subscribe(&quality_input, move |_, _, event: &InputEvent, cx| {
            let raw = quality_guard.read(cx).value().trim().to_string();
            let normalized = match raw.parse::<i32>() {
                Ok(value) if value < 1 => "1".to_string(),
                Ok(value) if value > 100 => "100".to_string(),
                Ok(_) => return,
                Err(_) => {
                    if matches!(event, InputEvent::Change) {
                        return;
                    }
                    "1".to_string()
                }
            };
            let _ = cx.update_window(quality_guard_window, |_, window, cx| {
                quality_guard.update(cx, |input, cx| input.set_value(normalized, window, cx));
            });
        });

        let filename_edited_sub = filename_edited.clone();
        let filename_subscription = cx.subscribe(&filename_input, move |_, _, event: &InputEvent, _| {
            if matches!(event, InputEvent::Change) {
                filename_edited_sub.set(true);
            }
        });
        self.export_subscriptions = vec![
            interval_subscription,
            quality_subscription,
            quality_guard_subscription,
            filename_subscription,
        ];

        window.open_dialog(cx, move |dialog, _, cx| {
            let loading = exporting.get();
            dialog
                .title(t(lang, "Export APNG", "导出 APNG"))
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
                            t(lang, "Frame interval", "帧间隔"),
                            t(lang, "Milliseconds, default 10 ms", "单位毫秒，默认 10 ms"),
                            &interval_input,
                            loading,
                            cx,
                        ))
                        .child(setting_field(
                            t(lang, "Quality", "质量"),
                            t(
                                lang,
                                "1-100; higher preserves more colors and produces a larger file",
                                "1-100；越高保留颜色越多，文件通常越大",
                            ),
                            &quality_input,
                            loading,
                            cx,
                        ))
                        .child(setting_field(
                            t(lang, "File name", "文件名"),
                            t(
                                lang,
                                "Editable. Defaults to name-interval-quality.png.",
                                "可编辑，默认为 名称-间隔-质量.png。",
                            ),
                            &filename_input,
                            loading,
                            cx,
                        ))
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(cx.theme().muted_foreground)
                                .child(export_output_notice(lang, &output_dimensions, frame_count)),
                        )
                )
                .footer(
                    h_flex()
                        .gap_2()
                        .justify_end()
                        .child(
                            Button::new("export-cancel")
                                .label(t(lang, "Cancel", "取消"))
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
                            let filename_input = filename_input.clone();
                            let frame_paths = frame_paths.clone();
                            let default_dir = default_dir.clone();
                            let ffmpeg = ffmpeg.clone();

                            Button::new("export-confirm")
                                .label(if loading {
                                    t(lang, "Exporting...", "导出中...")
                                } else {
                                    t(lang, "Export", "导出")
                                })
                                .primary()
                                .loading(loading)
                                .disabled(loading)
                                .on_click(move |_, window, cx| {
                                    if exporting.get() {
                                        return;
                                    }

                                    let Ok(interval) = interval_input.read(cx).value().trim().parse::<u16>() else {
                                        window.push_notification(
                                            Notification::error(t(
                                                lang,
                                                "Enter a valid frame interval",
                                                "请输入正确的帧间隔",
                                            )),
                                            cx,
                                        );
                                        return;
                                    };
                                    if !(1..=10_000).contains(&interval) {
                                        window.push_notification(
                                            Notification::error(
                                                t(
                                                    lang,
                                                    "Interval must be 1-10000 ms",
                                                    "间隔需在 1-10000 ms",
                                                ),
                                            ),
                                            cx,
                                        );
                                        return;
                                    }
                                    let quality_raw = quality_input.read(cx).value().trim().to_string();
                                    let quality = quality_raw
                                        .parse::<i32>()
                                        .unwrap_or(1)
                                        .clamp(1, 100) as u8;
                                    if quality.to_string() != quality_raw {
                                        let _ = quality_input.update(cx, |input, cx| {
                                            input.set_value(quality.to_string(), window, cx);
                                        });
                                    }
                                    let filename = filename_input
                                        .read(cx)
                                        .value()
                                        .trim()
                                        .to_string();
                                    if filename.is_empty() {
                                        window.push_notification(
                                            Notification::error(t(
                                                lang,
                                                "Enter a file name",
                                                "请输入文件名",
                                            )),
                                            cx,
                                        );
                                        return;
                                    }

                                    exporting.set(true);
                                    let _ = weak.update(cx, |_, cx| cx.notify());
                                    let receiver =
                                        cx.prompt_for_new_path(&default_dir, Some(&filename));
                                    let weak = weak.clone();
                                    let frame_paths = frame_paths.clone();
                                    let ffmpeg = ffmpeg.clone();
                                    let handle = main_window;

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
                                                        Notification::error(save_path_error(lang, error)),
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
                                                        Notification::error(save_path_error(lang, error)),
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
                                                        "{}",
                                                        match lang {
                                                            Language::Chinese => format!("APNG 已导出到 {}", output.display()),
                                                            Language::English => format!("APNG exported to {}", output.display()),
                                                        }
                                                    )),
                                                    cx,
                                                ),
                                                Err(error) => {
                                                    let message = friendly_export_error(lang, &error);
                                                    if error.contains("导出超时") {
                                                        window.push_notification(
                                                            Notification::warning(message),
                                                            cx,
                                                        );
                                                    } else {
                                                        window.push_notification(
                                                            Notification::error(message),
                                                            cx,
                                                        );
                                                    }
                                                }
                                            }
                                        });
                                    })
                                    .detach();
                                })
                        }),
                )
        });
    }

    fn open_settings_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let lang = self.language;
        let language_options = SearchableVec::new(vec![
            "中文".to_string(),
            "English".to_string(),
        ]);
        let language_select = cx.new(|cx| {
            SelectState::new(
                language_options,
                Some(IndexPath::new(match lang {
                    Language::Chinese => 0,
                    Language::English => 1,
                })),
                window,
                cx,
            )
        });
        let language_subscription = cx.subscribe(
            &language_select,
            move |this, _, event: &SelectEvent<SearchableVec<String>>, cx| {
                let SelectEvent::Confirm(Some(value)) = event else {
                    return;
                };
                let language = match value.as_str() {
                    "中文" => Language::Chinese,
                    _ => Language::English,
                };
                if this.language != language {
                    this.language = language;
                    Settings { language }.save();
                    cx.notify();
                }
            },
        );
        self.settings_subscription = Some(language_subscription);
        let app_version = env!("CARGO_PKG_VERSION").to_string();
        let update_available = updater::version();
        let automatic = updater::automatic();
        let updater_ready = updater::available();
        let ffmpeg_version = match &self.boot {
            BootState::Ready(environment) => Some(environment.version.clone()),
            _ => None,
        };
        let tab = Rc::new(Cell::new(SettingsTab::General));

        window.open_dialog(cx, move |dialog, _, cx| {
            dialog
                .title(t(lang, "Settings", "设置"))
                .margin_top(px(100.))
                .width(px(420.))
                .child(
                    v_flex()
                        .gap_4()
                        .child(
                            TabBar::new("settings-tabs")
                                .underline()
                                .selected_index(match tab.get() {
                                    SettingsTab::General => 0,
                                    SettingsTab::About => 1,
                                })
                                .child(Tab::new().label(t(lang, "General", "常规")))
                                .child(Tab::new().label(t(lang, "About", "关于")))
                                .on_click({
                                    let tab = tab.clone();
                                    move |index, window, _| {
                                        tab.set(if *index == 0 {
                                            SettingsTab::General
                                        } else {
                                            SettingsTab::About
                                        });
                                        window.refresh();
                                    }
                                }),
                        )
                        .when(tab.get() == SettingsTab::General, |this| {
                            this.child(
                                v_flex()
                                    .gap_4()
                                    .child(
                                        v_flex()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .text_size(px(13.))
                                                    .font_medium()
                                                    .child(t(lang, "Language", "界面语言")),
                                            )
                                            .child(
                                                Select::new(&language_select)
                                                    .w(px(180.))
                                                    .accessibility_label(t(
                                                        lang,
                                                        "Interface language",
                                                        "界面语言",
                                                    )),
                                            ),
                                    )
                                    .child(
                                        v_flex()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .text_size(px(13.))
                                                    .font_medium()
                                                    .child(t(lang, "Updates", "更新")),
                                            )
                                            .when_some(update_available.clone(), |this, version| {
                                                this.child(
                                                    div()
                                                        .text_size(px(12.))
                                                        .text_color(warm_accent())
                                                        .child(format!(
                                                            "{}: {}",
                                                            t(lang, "Update available", "有可用更新"),
                                                            version
                                                        )),
                                                )
                                            })
                                            .child(
                                                h_flex()
                                                    .gap_2()
                                                    .child(
                                                        Button::new("check-updates")
                                                            .label(t(lang, "Check for Updates", "检查更新"))
                                                            .disabled(!updater_ready)
                                                            .on_click(|_, _, _| updater::check()),
                                                    )
                                                    .child(
                                                        Button::new("automatic-updates")
                                                            .label(t(
                                                                lang,
                                                                if automatic { "Automatic checks: On" } else { "Automatic checks: Off" },
                                                                if automatic { "自动检查：开" } else { "自动检查：关" },
                                                            ))
                                                            .disabled(!updater_ready)
                                                            .on_click(move |_, window, cx| {
                                                                updater::set_automatic(!automatic);
                                                                window.close_dialog(cx);
                                                            }),
                                                    ),
                                            ),
                                    ),
                            )
                        })
                        .when(tab.get() == SettingsTab::About, |this| {
                            let components = [
                                "GPUI / gpui-kit 0.11.0",
                                "Sparkle 2.9.3",
                                "FFmpeg 7.1.1 (LGPL 2.1+)",
                            ];
                            this.child(
                                v_flex()
                                    .gap_2()
                                    .child(
                                        compact_detail_row(
                                            t(lang, "Version", "版本"),
                                            app_version.clone(),
                                            cx,
                                        )
                                    )
                                    .child(
                                        compact_detail_row(
                                            t(lang, "Dependency", "依赖"),
                                            format!(
                                                "{} ffmpeg {}",
                                                t(lang, "Bundled", "内置"),
                                                ffmpeg_version.clone().unwrap_or_else(|| {
                                                    t(lang, "Not detected", "未检测到").to_string()
                                                })
                                            ),
                                            cx,
                                        )
                                    )
                                    .child(
                                        div()
                                            .mt_1()
                                            .text_size(px(13.))
                                            .font_medium()
                                            .child(t(lang, "Open-source components", "开源组件")),
                                    )
                                    .children(components.into_iter().map(|component| {
                                        div()
                                            .text_size(px(12.))
                                            .line_height(px(18.))
                                            .text_color(cx.theme().muted_foreground)
                                            .child(format!("• {component}"))
                                    })),
                            )
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
        v_flex()
            .id("app-root")
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
    }
}

impl Builder {
    fn render_toolbar(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
            .child(
                div()
                    .id("open-settings")
                    .w(px(28.))
                    .h(px(28.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_settings_dialog(window, cx);
                    }))
                    .child(
                        Icon::new(Lucide::Settings)
                            .with_size(px(15.))
                            .text_color(cx.theme().muted_foreground),
                    ),
            )
    }

    fn render_checking(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let lang = self.language;
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
                    .child(t(lang, "Checking local environment", "正在检查本地环境")),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(cx.theme().muted_foreground)
                    .child(t(lang, "Verifying ffmpeg installation and version", "正在验证 ffmpeg 安装和版本")),
            )
    }

    fn render_failure(
        &mut self,
        message: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let lang = self.language;
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
                    .child(t(lang, "Unsupported system", "当前系统不受支持")),
            )
            .child(
                div()
                    .max_w(px(460.))
                    .text_align(TextAlign::Center)
                    .text_size(px(13.))
                    .line_height(px(19.))
                    .text_color(cx.theme().muted_foreground)
                    .child(localize_error(lang, message)),
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
        let lang = self.language;
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                v_flex()
                    .w(px(340.))
                    .h(px(230.))
                    .p_5()
                    .items_center()
                    .justify_center()
                    .relative()
                    .overflow_hidden()
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
                        svg()
                            .data(EMPTY_STATE_WATERMARK)
                            .absolute()
                            .right(px(-24.))
                            .bottom(px(-22.))
                            .w(px(210.))
                            .h(px(164.))
                            .text_color(warm_accent())
                            .opacity(0.12),
                    )
                    .child(
                        div()
                            .text_align(TextAlign::Center)
                            .text_size(px(16.))
                            .font_medium()
                            .mb(px(4.))
                            .child(t(lang, "Drop PNG frames into the window", "将 PNG 图片拖拽到窗口中")),
                    )
                    .child(
                        div()
                            .text_align(TextAlign::Center)
                            .text_size(px(12.))
                            .text_color(cx.theme().muted_foreground)
                            .mb(px(24.))
                            .child(t(lang, "List order is the animation playback order", "列表顺序即动画播放顺序")),
                    )
                    .child(
                        Button::new("add-images")
                            .label(t(lang, "Add Images", "添加图片"))
                            .primary()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_file_picker(window, cx);
                            })),
                    ),
            )
    }

    fn render_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let lang = self.language;
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
                            .child(format!("{} ({})", t(lang, "Frames", "图片"), self.frames.len())),
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
                    .on_hover(cx.listener(|this, hovering, window, cx| {
                        if *hovering {
                            let index = this.last_preview_index.or(this.selected);
                            if let Some(index) = index {
                                this.request_preview(Some(index), window, cx);
                            }
                        } else {
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
                        pill_button("add-more", t(lang, "Add Images", "添加图片"), false, cx)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_file_picker(window, cx);
                            })),
                    )
                    .child(
                        pill_button("export", t(lang, "Export", "导出"), true, cx)
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
        let lang = self.language;
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
                    language: lang,
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
                            .child(tag(t(lang, "Format: PNG", "格式：PNG"), cx))
                            .child(tag(
                                match lang {
                                    Language::Chinese => format!("像素：{}×{}", frame.info.width, frame.info.height),
                                    Language::English => format!("Pixels: {}×{}", frame.info.width, frame.info.height),
                                },
                                cx,
                            ))
                            .child(tag(
                                match lang {
                                    Language::Chinese => format!("大小：{}", format_bytes(frame.size_bytes)),
                                    Language::English => format!("Size: {}", format_bytes(frame.size_bytes)),
                                },
                                cx,
                            )),
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
        let lang = self.language;
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
                        .child(t(lang, "Select a frame to view details", "选择一张图片查看详情")),
                )
                .into_any_element();
        };
        let Some(frame) = self.frames.get(index) else {
            return div().flex_1().into_any_element();
        };

        let info = &frame.info;
        let not_recorded = t(lang, "Not recorded", "未记录");
        let mut details = vec![
            (t(lang, "Name", "名称"), frame.name.clone()),
            (t(lang, "Path", "路径"), frame.path.display().to_string()),
            (
                t(lang, "Size", "大小"),
                format!("{} ({} bytes)", format_bytes(frame.size_bytes), frame.size_bytes),
            ),
            (
                t(lang, "Dimensions", "尺寸"),
                format!("{} × {} px", info.width, info.height),
            ),
            (t(lang, "Format", "格式"), format!("PNG · bit depth {}", info.bit_depth)),
            (t(lang, "Color type", "颜色类型"), info.color_type_name().to_string()),
            (
                t(lang, "Compression", "压缩"),
                format!("deflate / filter {} / interlace {}", info.compression, info.filter),
            ),
            (t(lang, "Interlace", "隔行扫描"), info.interlace_name().to_string()),
            (
                t(lang, "DPI", "DPI"),
                match (info.dpi_x, info.dpi_y) {
                    (Some(x), Some(y)) => format!("{x:.1} × {y:.1}"),
                    _ => not_recorded.into(),
                },
            ),
            (
                t(lang, "Gamma", "Gamma"),
                info.gamma
                    .map(|value| format!("{value:.2}"))
                    .unwrap_or_else(|| not_recorded.into()),
            ),
            (
                t(lang, "sRGB intent", "sRGB 意图"),
                info.srgb_intent
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| not_recorded.into()),
            ),
            (
                t(lang, "Modified", "修改时间"),
                frame
                    .modified
                    .and_then(|time| format_time(lang, time))
                    .unwrap_or_else(|| not_recorded.into()),
            ),
            (
                t(lang, "PNG chunks", "PNG 块"),
                if info.chunks.is_empty() {
                    t(lang, "None", "无").into()
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
            details.push((t(lang, "Text metadata", "文本元数据"), text));
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
                            .child(frame_position(
                                lang,
                                index + 1,
                                self.frames.len(),
                            )),
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
    language: Language,
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
            .child(match self.language {
                Language::Chinese => format!("移动 {}", elide(&self.name, 24)),
                Language::English => format!("Move {}", elide(&self.name, 24)),
            })
    }
}

fn setting_field(
    label: &str,
    hint: &str,
    input: &Entity<InputState>,
    disabled: bool,
    cx: &App,
) -> impl IntoElement {
    v_flex()
        .gap_1()
        .child(
            h_flex()
                .w_full()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .min_w(px(84.))
                        .text_size(px(13.))
                        .font_medium()
                        .child(label.to_string()),
                )
                .child(Input::new(input).disabled(disabled).flex_1().min_w(px(0.))),
        )
        .child(
            div()
                .text_size(px(12.))
                .text_color(cx.theme().muted_foreground)
                .child(hint.to_string()),
        )
}

fn preview_details(frame: &Frame, lang: Language, cx: &App) -> Vec<AnyElement> {
    let info = &frame.info;
    let not_recorded = t(lang, "Not recorded", "未记录");
    let mut details = vec![
        (t(lang, "Name", "名称"), frame.name.clone()),
        (t(lang, "Path", "路径"), frame.path.display().to_string()),
        (
            t(lang, "File size", "文件大小"),
            format!("{} ({} bytes)", format_bytes(frame.size_bytes), frame.size_bytes),
        ),
        (
            t(lang, "Dimensions", "尺寸"),
            format!("{} × {} px", info.width, info.height),
        ),
        (t(lang, "Format", "格式"), "PNG".into()),
        (t(lang, "Bit depth", "位深"), info.bit_depth.to_string()),
        (t(lang, "Color type", "颜色类型"), info.color_type_name().to_string()),
        (
            t(lang, "Compression", "压缩"),
            format!("deflate, filter {}, interlace {}", info.compression, info.filter),
        ),
        (t(lang, "Interlace", "隔行扫描"), info.interlace_name().to_string()),
        (
            t(lang, "DPI", "DPI"),
            match (info.dpi_x, info.dpi_y) {
                (Some(x), Some(y)) => format!("{x:.1} × {y:.1}"),
                _ => not_recorded.into(),
            },
        ),
        (
            t(lang, "Gamma", "Gamma"),
            info.gamma
                .map(|value| format!("{value:.2}"))
                .unwrap_or_else(|| not_recorded.into()),
        ),
        (
            t(lang, "sRGB intent", "sRGB 意图"),
            info.srgb_intent
                .map(|value| value.to_string())
                .unwrap_or_else(|| not_recorded.into()),
        ),
        (
            t(lang, "Modified", "修改时间"),
            frame
                .modified
                .and_then(|time| format_time(lang, time))
                .unwrap_or_else(|| not_recorded.into()),
        ),
        (
            t(lang, "PNG chunks", "PNG 块"),
            if info.chunks.is_empty() {
                t(lang, "None", "无").into()
            } else {
                info.chunks.join(" · ")
            },
        ),
    ];
    if !info.text_entries.is_empty() {
        details.push((
            t(lang, "Text metadata", "文本元数据"),
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

fn sync_theme(window: Option<&mut Window>, cx: &mut App) {
    Theme::sync_system_appearance(window, cx);
    Theme::update(cx, |theme| {
        theme.overlay = gpui_kit::hsla(0., 0., 0., 0.64);
    });
}

fn t(lang: Language, en: &'static str, zh: &'static str) -> &'static str {
    match lang {
        Language::Chinese => zh,
        Language::English => en,
    }
}

fn added_frames_notice(lang: Language, count: usize) -> String {
    match lang {
        Language::Chinese => format!("已添加 {count} 张 PNG 图片"),
        Language::English => format!("Added {count} PNG frames"),
    }
}

fn skipped_frames_notice(lang: Language, count: usize) -> String {
    match lang {
        Language::Chinese => format!("已跳过 {count} 个非 PNG 或无法读取的文件"),
        Language::English => format!("Skipped {count} non-PNG or unreadable files"),
    }
}

fn picker_error(lang: Language, error: impl std::fmt::Display) -> String {
    match lang {
        Language::Chinese => format!("打开图片选择器失败：{error}"),
        Language::English => format!("Failed to open file picker: {error}"),
    }
}

fn save_path_error(lang: Language, error: impl std::fmt::Display) -> String {
    match lang {
        Language::Chinese => format!("选择保存位置失败：{error}"),
        Language::English => format!("Failed to choose save path: {error}"),
    }
}

fn delete_notice(lang: Language, name: &str) -> String {
    match lang {
        Language::Chinese => format!("“{name}” 将从序列帧列表中移除。"),
        Language::English => format!("“{name}” will be removed from the frame list."),
    }
}

fn export_output_notice(lang: Language, dimensions: &str, count: usize) -> String {
    match lang {
        Language::Chinese => format!("输出：{dimensions} · {count} 帧"),
        Language::English => format!("Output: {dimensions} · {count} frames"),
    }
}

fn frame_position(lang: Language, index: usize, total: usize) -> String {
    match lang {
        Language::Chinese => format!("第 {index} 帧，共 {total} 帧"),
        Language::English => format!("Frame {index} of {total}"),
    }
}

fn format_time(lang: Language, time: SystemTime) -> Option<String> {
    time.duration_since(UNIX_EPOCH).ok().map(|duration| {
        match lang {
            Language::Chinese => format!("{}（Unix 秒）", duration.as_secs()),
            Language::English => format!("{} (Unix seconds)", duration.as_secs()),
        }
    })
}

fn localize_error(lang: Language, error: &str) -> String {
    if lang == Language::Chinese {
        return error.to_string();
    }
    let replacements = [
        ("当前应用暂不支持当前系统。", "This app does not support the current system."),
        ("请先添加序列帧图片。", "Add PNG frames first."),
        ("帧间隔必须大于 0 ms。", "Frame interval must be greater than 0 ms."),
        ("创建临时目录失败", "Failed to create temp directory"),
        ("复制 ", "Failed to copy "),
        (" 失败", ""),
        ("启动 ffmpeg 失败", "Failed to start ffmpeg"),
        ("ffmpeg 导出失败", "ffmpeg export failed"),
    ];
    let mut text = error.to_string();
    for (from, to) in replacements {
        text = text.replace(from, to);
    }
    text
}

fn suggested_export_filename(
    stem: &str,
    interval_input: &Entity<InputState>,
    quality_input: &Entity<InputState>,
    cx: &App,
) -> String {
    let interval = interval_input
        .read(cx)
        .value()
        .trim()
        .parse::<u16>()
        .unwrap_or(10);
    let quality = quality_input
        .read(cx)
        .value()
        .trim()
        .parse::<u8>()
        .unwrap_or(80);
    format!("{stem}-{interval}-{quality}.png")
}

fn friendly_export_error(lang: Language, error: &str) -> String {
    let lower = error.to_ascii_lowercase();
    let reason = if error.contains("导出超时") {
        Some((
            "Export timed out and was stopped. Please retry, or use fewer frames.",
            "导出超时，已停止。请重试，或减少帧数后重试。",
        ))
    } else if lower.contains("cannot write more than one file")
        || lower.contains("sequence pattern")
    {
        Some((
            "The file name is not valid for an animated PNG.",
            "文件名不适用于动态 PNG，请调整后重试。",
        ))
    } else if lower.contains("permission denied") {
        Some((
            "No permission to write to the selected location.",
            "没有权限写入所选位置，请更换保存位置。",
        ))
    } else if lower.contains("no space left") {
        Some((
            "There is not enough disk space to export the animation.",
            "磁盘空间不足，无法完成导出。",
        ))
    } else if lower.contains("unknown encoder") || lower.contains("encoder not found") {
        Some((
            "This ffmpeg build cannot encode APNG.",
            "当前 ffmpeg 不支持导出 APNG，请安装完整版本。",
        ))
    } else {
        None
    };

    if let Some((en, zh)) = reason {
        return t(lang, en, zh).to_string();
    }
    t(lang, "Export failed. Please try again.", "导出失败，请重试。").to_string()
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
