use anyhow::bail;
use clipboard_rs::Clipboard;
use nectan_core::{
    code_lookup::{
        CodeLookupError, gen_code, issue_code, lookup_code,
    },
    protocol::{ConnectStatus, NectanState, connect},
};
use slint::{ComponentHandle, TimerMode, invoke_from_event_loop};
use std::sync::Arc;
use tracing::error;

use crate::{
    AddDeviceBridge, Error, LookupState, NectanSender,
    state::ui_state,
};

pub fn handle_add_device(w: &NectanSender, state: Arc<NectanState>) {
    let bridge = w.global::<AddDeviceBridge>();
    let ui_weak = w.as_weak();
    let s = state.clone();

    bridge.on_copy_code(|s| {
        let code = s.to_string();
        println!("Copied {code}",);
        ui_state().clipboard().set_text(code);
    });

    bridge.on_digits_only(|s| {
        s.chars()
            .filter(|c| c.is_ascii_digit())
            .take(6)
            .collect::<String>()
            .into()
    });

    bridge.on_format_code(|s| {
        let digits: String = s
            .chars()
            .filter(|c| c.is_ascii_digit())
            .take(6)
            .collect();
        if digits.len() > 3 {
            format!("{} {}", &digits[..3], &digits[3..]).into()
        } else {
            digits.into()
        }
    });

    bridge.on_request_code(move || {
        let ui = ui_state();
        let weak = ui_weak.clone();

        ui.timer().start(
            TimerMode::Repeated,
            std::time::Duration::from_millis(1000),
            move || {
                let Some(w) = weak.upgrade() else {
                    return;
                };
                let time_left =
                    w.global::<AddDeviceBridge>().get_time_left();
                w.global::<AddDeviceBridge>()
                    .set_time_left(time_left - 1);

                if time_left == 0 {
                    let ui = ui_state();
                    ui.timer().stop();
                }
            },
        );

        let code = gen_code();
        let code_split = format!("{} {}", &code[0..3], &code[3..6]);

        let state = s.clone();
        let ui_weak = ui_weak.clone();

        if let Some(ui) = ui_weak.upgrade() {
            let bridge = ui.global::<AddDeviceBridge>();
            bridge.set_our_code(code_split.into());
        }

        tokio::spawn(async move {
            let result = issue_code(&state, code).await;
            invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    let bridge = ui.global::<AddDeviceBridge>();
                    match result {
                        Ok(code) => {
                            let code_split = format!(
                                "{} {}",
                                &code[0..3],
                                &code[3..6]
                            );
                            bridge.set_our_code(code_split.into());
                        }
                        Err(e) => {
                            ui.global::<Error>().invoke_show_error(
                                "Error".into(),
                                e.to_string().into(),
                            );
                            // bridge.invoke_reset_add_remote();
                            bridge.set_open(false);
                            error!("Failed to issue code {e:?}");
                        }
                    }
                }
            })
            .unwrap();
        });
    });

    // Handle code lookup
    let s = state.clone();
    let ui_weak = w.as_weak();

    bridge.on_code_submitted(move |code| {
        let state = s.clone();
        let ui_weak = ui_weak.clone();

        tokio::spawn(async move {
            ui_weak.upgrade_in_event_loop(|ui| {
                ui.global::<AddDeviceBridge>()
                    .set_lookup_state(LookupState::Checking);
            })?;

            let result = lookup_code(code.to_string(), &state).await;
            if let Err(e) = result {
                match e {
                    CodeLookupError::NotFound => {
                        ui_weak.upgrade_in_event_loop(|ui| {
                            ui.global::<AddDeviceBridge>()
                                .set_lookup_state(
                                    LookupState::CodeNotFound,
                                );
                        })?;
                    }
                    CodeLookupError::ConnectionFailed => {
                        ui_weak.upgrade_in_event_loop(|ui| {
                            ui.global::<AddDeviceBridge>()
                                .set_lookup_state(
                                    LookupState::ConnectionFailed,
                                );
                        })?;
                    }
                    CodeLookupError::NectanService => {
                        ui_weak.upgrade_in_event_loop(|ui| {
                            ui.global::<Error>().invoke_show_error(
                                "Nectan failed".into(),
                                "Unexpected error.".into(),
                            );
                        })?;
                    }
                }
                bail!("Code err")
            }

            let addr = result.unwrap();
            ui_weak.upgrade_in_event_loop(|ui| {
                println!("Connecting");
                ui.global::<AddDeviceBridge>()
                    .set_lookup_state(LookupState::Connecting);
            })?;

            let mut rx = connect(state, addr);
            let result = rx.recv().await.unwrap();

            if let ConnectStatus::RequestDelivered = result {
                println!("Request delivered");
                ui_weak.upgrade_in_event_loop(move |ui| {
                    ui.global::<AddDeviceBridge>().set_lookup_state(
                        LookupState::WaitingForRemoteApproval,
                    );
                })?;
                let result = rx.recv().await.unwrap();
                match result {
                    ConnectStatus::UserRejected => {
                        println!("User rejected");
                        ui_weak.upgrade_in_event_loop(move |ui| {
                            ui.global::<AddDeviceBridge>()
                                .set_lookup_state(
                                    LookupState::Rejected,
                                );
                        })?;
                    }
                    ConnectStatus::Accepted(_device) => {
                        println!("Accepted");
                        ui_weak.upgrade_in_event_loop(move |ui| {
                            ui.global::<AddDeviceBridge>()
                                .set_lookup_state(
                                    LookupState::Connected,
                                );
                        })?;
                    }
                    ConnectStatus::AuthFailed => {
                        println!("AuthFailed");
                        ui_weak.upgrade_in_event_loop(move |ui| {
                            ui.global::<AddDeviceBridge>()
                                .set_lookup_state(
                                    LookupState::Rejected,
                                );
                        })?;
                    }
                    ConnectStatus::ConnectionFailed => {
                        println!("Conn failed");
                        ui_weak.upgrade_in_event_loop(move |ui| {
                            ui.global::<AddDeviceBridge>()
                                .set_lookup_state(
                                    LookupState::ConnectionFailed,
                                );
                        })?;
                    }
                    ConnectStatus::RequestDelivered
                    | ConnectStatus::AlreadyConnected => {
                        panic!("Impossible state")
                    }
                }
                return Ok(());
            }

            match result {
                ConnectStatus::ConnectionFailed => {
                    ui_weak.upgrade_in_event_loop(move |ui| {
                        println!("Connection failed");
                        ui.global::<AddDeviceBridge>()
                            .set_lookup_state(
                                LookupState::ConnectionFailed,
                            );
                    })?;
                }
                ConnectStatus::AlreadyConnected => {
                    println!("Already connected");
                    ui_weak.upgrade_in_event_loop(move |ui| {
                        ui.global::<AddDeviceBridge>()
                            .set_lookup_state(
                                LookupState::AlreadyConnected,
                            );
                    })?;
                }
                _ => {
                    panic!("Impossible state")
                }
            }
            Ok::<(), anyhow::Error>(())
        });
    });
}
