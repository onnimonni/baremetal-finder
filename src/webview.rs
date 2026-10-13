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

/// How long to wait hidden for Cloudflare's no-click check before asking the
/// user (it normally clears within a few seconds).
const QUIET: Duration = Duration::from_secs(3);
/// How long the user gets to click the check.
const INTERACTIVE: Duration = Duration::from_secs(120);

enum UserEvent {
    Html(String),
    Status(String),
}

/// Load `url` and return `document.documentElement.outerHTML` once the JS
/// expression `ready` is true (polled twice a second in every loaded page, so it
/// also runs on the page a challenge redirects to).
pub fn grab(url: &str, ready: &str) -> Result<String> {
    // BMF_WEBVIEW_DEBUG=1 shows the window from the start and logs titles.
    let debug = std::env::var_os("BMF_WEBVIEW_DEBUG").is_some();
    let interactive = std::io::stderr().is_terminal();
    let mut event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    #[cfg(target_os = "macos")]
    {
        // Background app: no Dock icon or menu bar unless a window is shown.
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        event_loop.set_activation_policy(ActivationPolicy::Accessory);
    }
    let proxy = event_loop.create_proxy();
    // Hidden unless the user must click Cloudflare's check (or debugging).
    let window = WindowBuilder::new()
        .with_title(format!("baremetal-finder: {url}"))
        .with_visible(debug)
        .with_focused(debug)
        .with_inner_size(LogicalSize::new(900.0, 700.0))
        .with_position(LogicalPosition::new(100.0, 100.0))
        .build(&event_loop)?;

    let poll = format!(
        "setInterval(() => {{ try {{ window.ipc.postMessage(({ready}) ? document.documentElement.outerHTML : 'status:' + document.title); }} catch (e) {{ window.ipc.postMessage('status:error ' + e); }} }}, 500);"
    );
    // BMF_WEBVIEW_FRESH=1 ignores saved cookies (forces the challenge).
    let fresh = std::env::var_os("BMF_WEBVIEW_FRESH").is_some();
    let webview = WebViewBuilder::new()
        .with_incognito(fresh)
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
    #[cfg(target_os = "macos")]
    {
        // WebKit suspends/throttles pages in hidden windows; let Cloudflare's
        // non-interactive challenge run to completion (macOS 14+).
        use objc2_web_kit::WKInactiveSchedulingPolicy;
        use wry::WebViewExtMacOS;
        let prefs = unsafe { _webview.webview().configuration().preferences() };
        unsafe { prefs.setInactiveSchedulingPolicy(WKInactiveSchedulingPolicy::None) };
    }
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
    event_loop.run_return(|event, target, control_flow| {
        *control_flow = ControlFlow::WaitUntil(deadline);
        match event {
            Event::UserEvent(UserEvent::Html(body)) => {
                html = Some(body);
                *control_flow = ControlFlow::Exit;
                return;
            }
            Event::UserEvent(UserEvent::Status(title)) => {
                if debug && title != last_title {
                    eprintln!("webview: {title}");
                }
                last_title = title;
            }
            _ => {}
        }
        let waited_out = Instant::now() >= deadline;
        // Keep polling hidden until the quiet period ends: the challenge
        // often clears without a click.
        if asked || !waited_out {
            if asked && waited_out {
                *control_flow = ControlFlow::Exit;
            }
            return;
        }
        if !interactive {
            *control_flow = ControlFlow::Exit;
            return;
        }
        // Show the page in front of the terminal, once.
        asked = true;
        deadline = Instant::now() + INTERACTIVE;
        #[cfg(target_os = "macos")]
        {
            use tao::platform::macos::{ActivationPolicy, EventLoopWindowTargetExtMacOS};
            target.set_activation_policy_at_runtime(ActivationPolicy::Regular);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = target;
        window.set_visible(true);
        window.set_always_on_top(true);
        window.set_focus();
        eprintln!(
            "Cloudflare wants a human check for {url}.\nClick \"Verify you are human\" in the opened window (waiting {}s)…",
            INTERACTIVE.as_secs()
        );
    });
    match html {
        Some(h) => Ok(h),
        None if !interactive => bail!(
            "{url} needs Cloudflare's human check; run `fetch --provider avoro` in a terminal once"
        ),
        None => bail!("{url} still on \"{last_title}\" after waiting"),
    }
}
