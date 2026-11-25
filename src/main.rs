mod app;
mod export;
mod frame;
mod png;

use gpui_kit::*;
use std::time::Duration;

actions!(
    easy_frame_actions,
    [QuitApp, MinimizeApp, CloseWindow, ZoomApp, ToggleFullscreenApp]
);

fn main() {
    gpui_kit::application()
        .with_quit_mode(QuitMode::Explicit)
        .with_assets(gpui_kit::assets::AllAssets)
        .run(|cx| {
            gpui_kit::init(cx);

            cx.bind_keys([
                KeyBinding::new("cmd-q", QuitApp, None),
                KeyBinding::new("cmd-m", MinimizeApp, None),
                KeyBinding::new("cmd-w", CloseWindow, None),
                KeyBinding::new("cmd-alt-m", ZoomApp, None),
                KeyBinding::new("ctrl-cmd-f", ToggleFullscreenApp, None),
            ]);

            let options = WindowOptions {
                window_bounds: Some(WindowBounds::centered(
                    None,
                    size(px(450.), px(740.)),
                    cx,
                )),
                window_min_size: Some(size(px(450.), px(560.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Easy Frame".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(18.), px(17.))),
                }),
                ..Default::default()
            };

            let (main_window, builder) = gpui_kit::open_window(options, cx, |window, cx| {
                window.activate_window();
                app::Builder::new(window, cx)
            })
            .expect("failed to open APNG Builder window");

            cx.on_action(|_: &QuitApp, cx| cx.quit());
            cx.on_action({
                let handle = main_window;
                move |_: &MinimizeApp, cx| {
                    let _ = handle.update(cx, |_, window, _| window.minimize_window());
                }
            });
            cx.on_action({
                let handle = main_window;
                move |_: &CloseWindow, cx| {
                    let _ = handle.update(cx, |_, window, _| window.minimize_window());
                }
            });
            cx.on_action({
                let handle = main_window;
                move |_: &ZoomApp, cx| {
                    let _ = handle.update(cx, |_, window, _| window.zoom_window());
                }
            });
            cx.on_action({
                let handle = main_window;
                move |_: &ToggleFullscreenApp, cx| {
                    let _ = handle.update(cx, |_, window, _| window.toggle_fullscreen());
                }
            });

            cx.set_menus([
                Menu::new("Easy Frame")
                    .items([MenuItem::action("Quit Easy Frame", QuitApp)]),
                Menu::new("File")
                    .items([MenuItem::action("Close Window", CloseWindow)]),
                Menu::new("Window").items([
                    MenuItem::action("Minimize", MinimizeApp),
                    MenuItem::action("Zoom", ZoomApp),
                    MenuItem::action("Enter Full Screen", ToggleFullscreenApp),
                ]),
            ]);

            cx.spawn(async move |cx| {
                loop {
                    cx.background_spawn(async {
                        std::thread::sleep(Duration::from_millis(300));
                    })
                    .await;
                    let _ = cx.update(|cx| {
                        if cx.active_window().is_none() {
                            let _ = builder.update(cx, |builder, cx| builder.close_preview(cx));
                        }
                    });
                }
            })
            .detach();
        });
}
