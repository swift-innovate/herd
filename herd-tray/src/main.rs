//! `herd-tray` — a Windows-first system-tray status viewer and launcher for the
//! Herd gateway.
//!
//! The interesting logic lives in [`state`] (icon/menu derivation) and [`status`]
//! (poll parsing), both pure and unit-tested. This file is the thin tao +
//! tray-icon + muda glue: it owns the event loop, forwards background poll
//! results and menu clicks through an [`EventLoopProxy`], and mutates the tray
//! only on the event-loop thread (a hard requirement of tray-icon/muda).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod autostart;
mod config;
mod icons;
mod single_instance;
mod state;
mod status;
mod supervise;

use crate::state::{gateway_control, next_state, GatewayControl, IconState, Mode, PollResult};
use crate::status::{ReqwestProbe, StatusProbe};
use crate::supervise::Supervisor;
use muda::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use std::time::Duration;
use tao::event::Event;
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::TrayIconBuilder;

/// Events funnelled onto the event-loop thread.
enum UserEvent {
    Poll(PollResult),
    MenuClick(muda::MenuId),
}

const POLL_INTERVAL: Duration = Duration::from_secs(5);

fn main() {
    // Single instance: a second launch just exits quietly. The guard is held for
    // the process lifetime (its Drop releases the mutex).
    let _guard: single_instance::InstanceGuard = match single_instance::acquire() {
        Some(g) => g,
        None => std::process::exit(0),
    };

    let flag = config::parse_gateway_flag(std::env::args().skip(1));
    let env = std::env::var(config::ENV_GATEWAY).ok();
    let gateway = config::resolve_gateway(flag, env);

    // Decide attach vs supervise from a single launch probe.
    let mut supervisor = Supervisor::new();
    let mode = match ReqwestProbe::new().probe(&gateway) {
        PollResult::Up { .. } => Mode::Attach,
        PollResult::Unreachable => {
            if let Err(e) = supervisor.spawn() {
                eprintln!("herd-tray: could not start gateway: {e:#}");
            }
            Mode::Supervise
        }
    };

    if let Err(e) = run(gateway, mode, supervisor) {
        eprintln!("herd-tray: fatal: {e:#}");
        std::process::exit(1);
    }
}

fn run(gateway: String, mode: Mode, mut supervisor: Supervisor) -> anyhow::Result<()> {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    // Route muda menu activations onto the event loop.
    let menu_proxy = proxy.clone();
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        let _ = menu_proxy.send_event(UserEvent::MenuClick(e.id));
    }));

    // Build the menu. Item handles are retained so we can toggle them live.
    let supervising = matches!(mode, Mode::Supervise);
    let menu = Menu::new();
    let open_item = MenuItem::new("Open Dashboard", true, None);
    let models_item = MenuItem::new("Models…", true, None);
    let start_item = MenuItem::new("Start gateway", false, None);
    let stop_item = MenuItem::new("Stop gateway", true, None);
    let autostart_item = CheckMenuItem::new("Start at login", true, autostart::is_enabled(), None);
    let quit_item = MenuItem::new("Quit", true, None);

    menu.append(&open_item)?;
    menu.append(&models_item)?;
    menu.append(&PredefinedMenuItem::separator())?;
    if supervising {
        menu.append(&start_item)?;
        menu.append(&stop_item)?;
        menu.append(&PredefinedMenuItem::separator())?;
    }
    if autostart::supported() {
        menu.append(&autostart_item)?;
        menu.append(&PredefinedMenuItem::separator())?;
    }
    menu.append(&quit_item)?;

    // Bootstrap gray until the first poll resolves.
    let mut current = IconState::Gray;
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Herd")
        .with_icon(icons::tray_icon_for(current)?)
        .build()?;

    // Background poll thread → event loop.
    let poll_gateway = gateway.clone();
    std::thread::spawn(move || {
        let probe = ReqwestProbe::new();
        loop {
            let result = probe.probe(&poll_gateway);
            if proxy.send_event(UserEvent::Poll(result)).is_err() {
                break; // event loop gone
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    });

    // Own copies of the ids for click matching.
    let open_id = open_item.id().clone();
    let models_id = models_item.id().clone();
    let start_id = start_item.id().clone();
    let stop_id = stop_item.id().clone();
    let autostart_id = autostart_item.id().clone();
    let quit_id = quit_item.id().clone();

    let base = gateway.trim_end_matches('/').to_string();
    let dashboard_url = format!("{base}/dashboard");
    // Deep-links to the dashboard's Settings tab (the model picker lands here once
    // #G3 ships that tab; until then it opens the dashboard).
    let models_url = format!("{base}/dashboard#settings");

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::UserEvent(UserEvent::Poll(poll)) => {
                let alive = supervisor.is_alive();
                let st = next_state(poll, supervising, alive);
                if st != current {
                    current = st;
                    match icons::tray_icon_for(st) {
                        Ok(icon) => {
                            if let Err(e) = tray.set_icon(Some(icon)) {
                                eprintln!("[herd-tray] set_icon failed: {e}");
                            }
                        }
                        Err(e) => eprintln!("[herd-tray] icon build failed: {e}"),
                    }
                }
                // Reflect gateway liveness in the Start/Stop items.
                if supervising {
                    match gateway_control(mode, alive) {
                        GatewayControl::ShowStart => {
                            start_item.set_enabled(true);
                            stop_item.set_enabled(false);
                        }
                        GatewayControl::ShowStop => {
                            start_item.set_enabled(false);
                            stop_item.set_enabled(true);
                        }
                        GatewayControl::None => {}
                    }
                }
            }
            Event::UserEvent(UserEvent::MenuClick(id)) => {
                if id == open_id {
                    let _ = open::that(&dashboard_url);
                } else if id == models_id {
                    let _ = open::that(&models_url);
                } else if id == start_id {
                    if let Err(e) = supervisor.spawn() {
                        eprintln!("herd-tray: start gateway failed: {e:#}");
                    }
                } else if id == stop_id {
                    supervisor.kill();
                } else if id == autostart_id {
                    let want = autostart_item.is_checked();
                    if let Err(e) = autostart::set(want) {
                        eprintln!("herd-tray: autostart toggle failed: {e:#}");
                        autostart_item.set_checked(!want); // revert the UI
                    }
                } else if id == quit_id {
                    supervisor.kill(); // attached gateways are left running
                    *control_flow = ControlFlow::Exit;
                }
            }
            _ => {}
        }
    });
}
