//! Fetch a page through the operating system's web engine (WebKit on macOS
//! and Linux, WebView2 on Windows) via `wry`, for sites behind Cloudflare
//! challenges that no HTTP client passes. The engine runs in-process from a
//! small off-screen window; it needs the main thread and, on Linux, a display.
//!
//! When Cloudflare insists on its interactive "verify you are human" check,
//! the window is moved on-screen for the user to click it. That check is
//! never automated. The resulting clearance cookie is kept by the OS web
//! engine, so later runs pass silently until it expires.

use std::io::IsTerminal;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use tao::dpi::{LogicalPosition, LogicalSize};
use tao::event::Event;
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::platform::run_return::EventLoopExtRunReturn;
use tao::window::WindowBuilder;
use wry::WebViewBuilder;

/// How long to wait off-screen before asking the user for the check.
const QUIET: Duration = Duration::from_secs(20);
/// How long the user gets to click the check.
const INTERACTIVE: Duration = Duration::from_secs(120);

enum UserEvent {
    Html(String),
    Status(String),
}

/// Load `url` and return `document.documentElement.outerHTML` once the JS
/// expression `ready` is true (polled every second in every loaded page, so it
/// also runs on the page a challenge redirects to).
pub fn grab(url: &str, ready: &str) -> Result<String> {
    // BMF_WEBVIEW_DEBUG=1 shows the window from the start and logs titles.
    let debug = std::env::var_os("BMF_WEBVIEW_DEBUG").is_some();
    let interactive = std::io::stderr().is_terminal();
    let mut event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    // A hidden window gets its page throttled by the OS and the challenge
    // never finishes, so keep it "visible" but off-screen.
    let window = WindowBuilder::new()
        .with_title(format!("baremetal-finder: {url}"))
        .with_focused(debug)
        .with_inner_size(LogicalSize::new(900.0, 700.0))
        .with_position(if debug {
            LogicalPosition::new(100.0, 100.0)
        } else {
            LogicalPosition::new(-10_000.0, -10_000.0)
        })
        .build(&event_loop)?;

    let poll = format!(
        "setInterval(() => {{ try {{ window.ipc.postMessage(({ready}) ? document.documentElement.outerHTML : 'status:' + document.title); }} catch (e) {{ window.ipc.postMessage('status:error ' + e); }} }}, 1000);"
    );
    let webview = WebViewBuilder::new()
        .with_url(url)
        .with_initialization_script(&poll)
        .with_ipc_handler(move |req| {
            let body = req.into_body();
            let event = match body.strip_prefix("status:") {
                Some(s) => UserEvent::Status(s.to_string()),
                None => UserEvent::Html(body),
            };
            let _ = proxy.send_event(event);
        });

    #[cfg(not(target_os = "linux"))]
    let _webview = webview.build(&window)?;
    #[cfg(target_os = "linux")]
    let _webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        webview.build_gtk(
            window
                .default_vbox()
                .expect("tao windows have a vbox on Linux"),
        )?
    };

    let mut deadline = Instant::now() + QUIET;
    let mut asked = false;
    let mut html = None;
    let mut last_title = String::new();
    event_loop.run_return(|event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(deadline);
        match event {
            Event::UserEvent(UserEvent::Html(body)) => {
                html = Some(body);
                *control_flow = ControlFlow::Exit;
            }
            Event::UserEvent(UserEvent::Status(title)) => {
                if debug && title != last_title {
                    eprintln!("webview: {title}");
                }
                last_title = title;
            }
            _ if Instant::now() < deadline => {}
            // Still on the challenge after the quiet period: ask once.
            _ if !asked && interactive => {
                asked = true;
                deadline = Instant::now() + INTERACTIVE;
                window.set_outer_position(LogicalPosition::new(100.0, 100.0));
                window.set_focus();
                eprintln!(
                    "Cloudflare wants a human check for {url}.\nClick \"Verify you are human\" in the opened window (waiting {}s)…",
                    INTERACTIVE.as_secs()
                );
            }
            _ => *control_flow = ControlFlow::Exit,
        }
    });
    match html {
        Some(h) => Ok(h),
        None if !interactive => bail!(
            "{url} needs Cloudflare's human check; run `fetch --provider avoro` in a terminal once"
        ),
        None => bail!("{url} still on \"{last_title}\" after waiting"),
    }
}
